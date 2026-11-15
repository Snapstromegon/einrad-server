//! `JuryQueries` for SQLite: judges, their registrations and the Juryplan
//! (#225).

use std::collections::HashMap;

use db_core::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{ClubId, FinalId, HeatId, JudgeId, JuryUnitId},
    jury::{
        self, JudgeFacts, JudgeTask, JuryUnitCategory, JuryUnitFeed, JuryUnitFinalType,
        JuryUnitKind, JuryUnitRow, JuryUnitRows, PlannedBlock, unit_registrable,
    },
    queries::jury::{
        ClubJudge, JuryAssignment, JuryPlan, JuryQueries, JuryUnit, ListClubJudges, ListJuryPlan,
        ListedRegistration, UnmappedRegistration, sees_whole_jury_plan,
    },
    timeplan::{Audience, EntryKind},
};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

use super::{SqliteRepo, read_tx};
use crate::{registration::club_access, timeplan::load_for};

#[async_trait::async_trait]
impl JuryQueries for SqliteRepo {
    async fn list_club_judges(
        &self,
        actor: &Actor,
        query: ListClubJudges,
    ) -> Result<Vec<ClubJudge>, QueryError> {
        read_tx!(self, tx, {
            club_access(&mut tx, actor, query.club_id).await?;
            Ok(list_judges(&mut tx, Some(query.club_id)).await?)
        })
    }

    async fn list_judges(&self, _actor: &Actor) -> Result<Vec<ClubJudge>, QueryError> {
        read_tx!(self, tx, { Ok(list_judges(&mut tx, None).await?) })
    }

    async fn list_jury_plan(
        &self,
        actor: &Actor,
        query: ListJuryPlan,
    ) -> Result<JuryPlan, QueryError> {
        let whole_plan = sees_whole_jury_plan(actor);
        read_tx!(self, tx, {
            match query.club_id {
                Some(club_id) => club_access(&mut tx, actor, club_id).await?,
                None if whole_plan => {}
                None => return Err(QueryError::Forbidden),
            }
            let published_at = jury_plan_published_at(&mut tx).await?;
            if published_at.is_none() && !whole_plan {
                return Ok(JuryPlan {
                    published_at,
                    assignments: vec![],
                });
            }
            let assignments = list_jury_assignments(&mut tx, query.club_id).await?;
            Ok(JuryPlan {
                published_at,
                assignments,
            })
        })
    }

    async fn list_jury_units(&self, _actor: &Actor) -> Result<Vec<JuryUnit>, QueryError> {
        // The units, the categories and the timeplan are read in one
        // snapshot.
        read_tx!(self, tx, {
            let rows = jury_unit_rows(&mut tx).await?;
            // Without a timeplan (or without a start time for its first
            // block) there are no days.
            let timeplan = load_for(&mut tx, Audience::OrganizerTeam).await?;
            let blocks = match timeplan.planned_starts() {
                Some(starts) => timeplan
                    .entries
                    .iter()
                    .zip(starts)
                    .filter(|(entry, _)| entry.kind == EntryKind::Acts)
                    .map(|(entry, start)| PlannedBlock {
                        home_category: entry.category.clone(),
                        final_id: entry.final_id,
                        day: start.date(),
                    })
                    .collect(),
                None => {
                    tracing::warn!("No days for the jury units, the timeplan can't be predicted.");
                    vec![]
                }
            };
            Ok(jury::jury_units(rows, &blocks))
        })
    }
}

/// The jury units (every heat's Vorlauf and every final) with the settings
/// and the data their discipline and day are derived from.
async fn jury_unit_rows(conn: &mut SqliteConnection) -> sqlx::Result<JuryUnitRows> {
    let units = sqlx::query!(
        r#"
        SELECT
            id as "id!: JuryUnitId",
            kind as "kind!: JuryUnitKind",
            name as "name!",
            code as "code!",
            description,
            registrable_setting as "registrable_setting!: bool",
            skipped as "skipped!: bool",
            deployments as "deployments!: i64",
            needed_technical as "needed_technical!: i64",
            needed_performance as "needed_performance!: i64",
            needed_dismounts as "needed_dismounts!: i64"
        FROM view_jury_unit
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryUnitRow {
        id: row.id,
        kind: row.kind,
        name: row.name,
        code: row.code,
        description: row.description,
        registrable: unit_registrable(row.registrable_setting, row.skipped),
        registrable_setting: row.registrable_setting,
        skipped: row.skipped,
        deployments: row.deployments,
        needed_technical: row.needed_technical,
        needed_performance: row.needed_performance,
        needed_dismounts: row.needed_dismounts,
    })
    .collect();
    let categories = sqlx::query!(
        r#"SELECT name as "name!", heat_id as "heat_id: HeatId", act_type as "act_type!: ActType"
            FROM categories ORDER BY "order" NULLS FIRST, name"#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryUnitCategory {
        name: row.name,
        heat_id: row.heat_id,
        act_type: row.act_type,
    })
    .collect();
    let feeds = sqlx::query!(
        r#"SELECT heat_id as "heat_id!: HeatId", MIN(final_id) as "final_id!: FinalId"
        FROM final_slots WHERE heat_id IS NOT NULL GROUP BY heat_id"#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryUnitFeed {
        heat_id: row.heat_id,
        final_id: row.final_id,
    })
    .collect();
    let final_types = sqlx::query!(
        r#"SELECT id as "id!: FinalId", act_type as "act_type!: ActType" FROM finals"#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryUnitFinalType {
        id: row.id,
        act_type: row.act_type,
    })
    .collect();
    Ok(JuryUnitRows {
        units,
        categories,
        feeds,
        final_types,
    })
}

/// Judges with their registrations: of one club, or of all clubs.
async fn list_judges(
    conn: &mut SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<Vec<ClubJudge>> {
    let mut registrations: HashMap<JudgeId, Vec<ListedRegistration>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration.judge_id as "judge_id!: JudgeId",
            judge_registration.unit_id as "unit_id!: JuryUnitId",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            judge_registration.task as "task!: JudgeTask",
            view_jury_unit.registrable_setting as "registrable_setting!: bool",
            view_jury_unit.skipped as "skipped!: bool"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN view_jury_unit ON view_jury_unit.id = judge_registration.unit_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY view_jury_unit.kind DESC, view_jury_unit.name, judge_registration.task
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        registrations
            .entry(row.judge_id)
            .or_default()
            .push(ListedRegistration {
                unit_kind: row.unit_kind,
                unit_id: row.unit_id,
                task: row.task,
                flagged: !unit_registrable(row.registrable_setting, row.skipped),
            });
    }
    let mut unmapped: HashMap<JudgeId, Vec<UnmappedRegistration>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration_unmapped.id as "id!: i64",
            judge_registration_unmapped.judge_id as "judge_id!: JudgeId",
            judge_registration_unmapped.old_unit as "old_unit!",
            judge_registration_unmapped.task as "task!: JudgeTask"
        FROM judge_registration_unmapped
        JOIN judge ON judge.id = judge_registration_unmapped.judge_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY judge_registration_unmapped.id
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        unmapped
            .entry(row.judge_id)
            .or_default()
            .push(UnmappedRegistration {
                id: row.id,
                old_unit: row.old_unit,
                task: row.task,
            });
    }
    let mut unit_registrations = crate::fees::judge_registrations(&mut *conn, club_id).await?;
    let judges = sqlx::query!(
        r#"
        SELECT
            judge.id as "id!: JudgeId",
            judge.club_id as "club_id!: ClubId",
            clubs.name as club_name,
            judge.firstname,
            judge.lastname,
            judge.birthdate as "birthdate: OffsetDateTime",
            judge.is_trained,
            judge.max_deployments,
            judge.note
        FROM judge
        JOIN clubs ON clubs.id = judge.club_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY clubs.name, judge.lastname, judge.firstname
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(judges
        .into_iter()
        .map(|row| {
            let facts = JudgeFacts {
                club_id: row.club_id,
                is_trained: row.is_trained,
                max_deployments: row.max_deployments,
                registrations: unit_registrations.remove(&row.id).unwrap_or_default(),
            };
            ClubJudge {
                registrations: registrations.remove(&row.id).unwrap_or_default(),
                unmapped_registrations: unmapped.remove(&row.id).unwrap_or_default(),
                id: row.id,
                club_id: row.club_id,
                club_name: row.club_name,
                firstname: row.firstname,
                lastname: row.lastname,
                birthdate: row.birthdate,
                is_trained: row.is_trained,
                max_deployments: row.max_deployments,
                note: row.note,
                registered_deployments: facts.registered_deployments(),
                counted_deployments: facts.counted_deployments(),
            }
        })
        .collect())
}

/// When the Juryplan was published; `None` while it wasn't (or there is no
/// competition).
async fn jury_plan_published_at(
    conn: &mut SqliteConnection,
) -> sqlx::Result<Option<OffsetDateTime>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT jury_plan_published_at as "published_at: OffsetDateTime" FROM competition ORDER BY start_date DESC LIMIT 1"#
    )
    .fetch_optional(&mut *conn)
    .await?
    .flatten())
}

/// The Juryplan's assignments: of one club's judges, or of all.
async fn list_jury_assignments(
    conn: &mut SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<Vec<JuryAssignment>> {
    sqlx::query_as!(
        JuryAssignment,
        r#"
        SELECT
            jury_assignments.judge_id as "judge_id!: JudgeId",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            jury_assignments.unit_id as "unit_id!: JuryUnitId",
            jury_assignments.task as "task!: JudgeTask"
        FROM jury_assignments
        JOIN judge ON judge.id = jury_assignments.judge_id
        JOIN view_jury_unit ON view_jury_unit.id = jury_assignments.unit_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY view_jury_unit.kind DESC, view_jury_unit.name, jury_assignments.task, judge.lastname, judge.firstname
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await
}
