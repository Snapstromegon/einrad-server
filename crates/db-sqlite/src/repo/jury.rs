//! Judges, their registrations and the Juryplan (#225).

use std::collections::{HashMap, HashSet};

use db_core::{
    DbError, JuryRepo,
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    ids::{ClubId, FinalId, HeatId, JudgeId, JuryUnitId},
    jury::{
        ClubJudge, JudgeAgeRule, JudgeFields, JudgeTask, JuryAssignment, JuryUnitCategory,
        JuryUnitFeed, JuryUnitFields, JuryUnitFinalType, JuryUnitKind, JuryUnitRow, JuryUnitRows,
        ListedRegistration, NOT_REGISTRABLE_MESSAGE, RemapOutcome, RemapProblem, RemapRegistration,
        UnmappedRegistration, counted_deployments, parse_unit_label, unit_columns,
    },
    people,
};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

use super::SqliteRepo;
use crate::registration::{club_access, map_duplicate};

fn not_registrable() -> RegistrationError {
    RegistrationError::Rejected(NOT_REGISTRABLE_MESSAGE.to_string())
}

/// Whether judges can register for the heat or final now; `None` for an
/// unknown unit.
async fn unit_is_registrable(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    id: JuryUnitId,
) -> sqlx::Result<Option<bool>> {
    sqlx::query_scalar!(
        r#"SELECT registrable as "registrable!: bool" FROM view_jury_unit WHERE kind = $1 AND id = $2"#,
        kind,
        id
    )
    .fetch_optional(conn)
    .await
}

/// Loads the club of a judge and checks that the actor may manage it.
async fn judge_access(
    conn: &mut SqliteConnection,
    actor: &Actor,
    judge_id: JudgeId,
) -> Result<ClubId, RegistrationError> {
    let club_id = sqlx::query_scalar!(
        r#"SELECT club_id as "club_id!: ClubId" FROM judge WHERE id = $1"#,
        judge_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    club_access(conn, actor, club_id).await?;
    Ok(club_id)
}

/// The jury's age rule of the current competition; `None` without a
/// competition or without jury rules.
async fn judge_age_rule(conn: &mut SqliteConnection) -> sqlx::Result<Option<JudgeAgeRule>> {
    Ok(crate::competition::current(conn)
        .await?
        .and_then(|competition| {
            competition.jury.map(|jury| JudgeAgeRule {
                min_age: jury.min_age,
                competition_start: competition.start_date,
            })
        }))
}

/// Fails with a German message if the club already registered a judge with
/// this name (ignoring case and spaces). `except_id` excludes the judge being
/// edited.
async fn ensure_judge_name_is_free(
    conn: &mut SqliteConnection,
    club_id: ClubId,
    firstname: &str,
    lastname: &str,
    except_id: JudgeId,
) -> Result<(), RegistrationError> {
    let names = sqlx::query!(
        "SELECT firstname, lastname FROM judge WHERE club_id = $1 AND id IS NOT $2",
        club_id,
        except_id,
    )
    .fetch_all(&mut *conn)
    .await?;
    let key = people::person_key(firstname, lastname);
    match names
        .iter()
        .find(|row| people::person_key(&row.firstname, &row.lastname) == key)
    {
        Some(row) => Err(RegistrationError::Rejected(
            people::already_registered_message(&row.firstname, &row.lastname),
        )),
        None => Ok(()),
    }
}

/// Writes a judge's fields and replaces their registrations; an untrained
/// judge also loses their Technik and Performance assignments. Unknown jury
/// units, new registrations for a unit that takes none (D6; the judge's
/// existing ones may stay) and duplicate names are refused with a German
/// message.
async fn save_judge(
    conn: &mut SqliteConnection,
    judge_id: JudgeId,
    club_id: ClubId,
    fields: &JudgeFields,
) -> Result<(), RegistrationError> {
    ensure_judge_name_is_free(conn, club_id, &fields.firstname, &fields.lastname, judge_id).await?;
    sqlx::query!(
        r#"
        INSERT INTO judge (
            id, club_id, firstname, lastname, birthdate, is_trained, max_deployments, note
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (id) DO UPDATE SET
            firstname = excluded.firstname,
            lastname = excluded.lastname,
            birthdate = excluded.birthdate,
            is_trained = excluded.is_trained,
            max_deployments = excluded.max_deployments,
            note = excluded.note
        "#,
        judge_id,
        club_id,
        fields.firstname,
        fields.lastname,
        fields.birthdate,
        fields.is_trained,
        fields.max_deployments,
        fields.note,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| map_duplicate(e, &fields.firstname, &fields.lastname))?;
    let existing: HashSet<(JuryUnitKind, JuryUnitId, JudgeTask)> = sqlx::query!(
        r#"
        SELECT
            CASE WHEN heat_id IS NULL THEN 'final' ELSE 'heat' END as "kind!: JuryUnitKind",
            unit_id as "unit_id!: JuryUnitId",
            task as "task!: JudgeTask"
        FROM judge_registration WHERE judge_id = $1
        "#,
        judge_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| (row.kind, row.unit_id, row.task))
    .collect();
    sqlx::query!(
        "DELETE FROM judge_registration WHERE judge_id = $1",
        judge_id
    )
    .execute(&mut *conn)
    .await?;
    if !fields.is_trained {
        // Untrained people only count dismounts, also in the Juryplan.
        sqlx::query!(
            "DELETE FROM jury_assignments WHERE judge_id = $1 AND task != 'dismounts'",
            judge_id
        )
        .execute(&mut *conn)
        .await?;
    }
    for registration in &fields.registrations {
        let registrable =
            unit_is_registrable(conn, registration.unit_kind, registration.unit_id).await?;
        match registrable {
            None => {
                return Err(RegistrationError::Rejected(
                    "Eine gewählte Vorlaufklasse oder ein gewähltes Finale gibt es nicht mehr. Bitte neu laden."
                        .to_string(),
                ));
            }
            // Only what the judge already has there may stay, per task.
            Some(false)
                if !existing.contains(&(
                    registration.unit_kind,
                    registration.unit_id,
                    registration.task,
                )) =>
            {
                return Err(not_registrable());
            }
            Some(_) => {}
        }
        let (heat_id, final_id) = unit_columns(registration.unit_kind, registration.unit_id);
        sqlx::query!(
            "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)",
            judge_id,
            heat_id,
            final_id,
            registration.task,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl JuryRepo for SqliteRepo {
    async fn jury_unit_rows(&self) -> Result<JuryUnitRows, DbError> {
        let pool = self.pool().await;
        let mut tx = pool.begin().await?;
        let units = sqlx::query!(
            r#"
            SELECT
                id as "id!: JuryUnitId",
                kind as "kind!: JuryUnitKind",
                name as "name!",
                code as "code!",
                description,
                registrable as "registrable!: bool",
                registrable_setting as "registrable_setting!: bool",
                skipped as "skipped!: bool",
                deployments as "deployments!: i64",
                needed_technical as "needed_technical!: i64",
                needed_performance as "needed_performance!: i64",
                needed_dismounts as "needed_dismounts!: i64"
            FROM view_jury_unit
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| JuryUnitRow {
            id: row.id,
            kind: row.kind,
            name: row.name,
            code: row.code,
            description: row.description,
            registrable: row.registrable,
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
        .fetch_all(&mut *tx)
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
        .fetch_all(&mut *tx)
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
        .fetch_all(&mut *tx)
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

    async fn list_judges(&self, club_id: Option<ClubId>) -> Result<Vec<ClubJudge>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        let mut registrations: HashMap<JudgeId, Vec<ListedRegistration>> = HashMap::new();
        for row in sqlx::query!(
            r#"
            SELECT
                judge_registration.judge_id as "judge_id!: JudgeId",
                judge_registration.unit_id as "unit_id!: JuryUnitId",
                view_jury_unit.kind as "unit_kind!: JuryUnitKind",
                judge_registration.task as "task!: JudgeTask",
                NOT view_jury_unit.registrable as "flagged!: bool"
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
                    flagged: row.flagged,
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
                judge.note,
                (
                    SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                    WHERE c.id IN (
                        SELECT unit_id FROM judge_registration
                        WHERE judge_id = judge.id
                    )
                ) as "registered_deployments!: i64",
                (
                    SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                    WHERE c.id IN (
                        SELECT unit_id FROM judge_registration
                        WHERE judge_id = judge.id AND task != 'dismounts'
                    )
                ) as "countable_deployments!: i64"
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
            .map(|row| ClubJudge {
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
                registered_deployments: row.registered_deployments,
                counted_deployments: counted_deployments(
                    row.is_trained,
                    row.countable_deployments,
                    row.max_deployments,
                ),
            })
            .collect())
    }

    async fn add_judge(
        &self,
        actor: &Actor,
        club_id: ClubId,
        fields: JudgeFields,
    ) -> Result<JudgeId, RegistrationError> {
        let mut tx = self.begin_write().await?;
        club_access(&mut tx, actor, club_id).await?;
        let rule = judge_age_rule(&mut tx).await?;
        let fields = fields
            .normalize(rule)
            .map_err(RegistrationError::Rejected)?;
        let judge_id = JudgeId::new();
        save_judge(&mut tx, judge_id, club_id, &fields).await?;
        tx.commit().await?;
        Ok(judge_id)
    }

    async fn edit_judge(
        &self,
        actor: &Actor,
        judge_id: JudgeId,
        fields: JudgeFields,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let club_id = judge_access(&mut tx, actor, judge_id).await?;
        let rule = judge_age_rule(&mut tx).await?;
        let fields = fields
            .normalize(rule)
            .map_err(RegistrationError::Rejected)?;
        save_judge(&mut tx, judge_id, club_id, &fields).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_judge(
        &self,
        actor: &Actor,
        judge_id: JudgeId,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        judge_access(&mut tx, actor, judge_id).await?;
        // The registrations go with it (foreign key).
        sqlx::query!("DELETE FROM judge WHERE id = $1", judge_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn jury_plan_published_at(&self) -> Result<Option<OffsetDateTime>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_scalar!(
            r#"SELECT jury_plan_published_at as "published_at: OffsetDateTime" FROM competition ORDER BY start_date DESC LIMIT 1"#
        )
        .fetch_optional(&pool)
        .await?
        .flatten())
    }

    async fn list_jury_assignments(
        &self,
        club_id: Option<ClubId>,
    ) -> Result<Vec<JuryAssignment>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_as!(
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
        .fetch_all(&pool)
        .await?)
    }

    async fn assign_judge(&self, assignment: &JuryAssignment) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let is_trained = sqlx::query_scalar!(
            "SELECT is_trained FROM judge WHERE id = $1",
            assignment.judge_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RegistrationError::NotFound)?;
        if !is_trained && assignment.task != JudgeTask::Dismounts {
            return Err(RegistrationError::Rejected(
                "Nicht geschulte Personen können nur Abstiege zählen.".to_string(),
            ));
        }
        let (heat_id, final_id) = unit_columns(assignment.unit_kind, assignment.unit_id);
        let result = sqlx::query!(
            "INSERT INTO jury_assignments (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)",
            assignment.judge_id,
            heat_id,
            final_id,
            assignment.task,
        )
        .execute(&mut *tx)
        .await;
        match result {
            Ok(_) => {}
            Err(e)
                if e.as_database_error()
                    .is_some_and(|e| e.is_unique_violation()) =>
            {
                return Err(RegistrationError::Rejected(
                    "Die JurorIn ist hier schon eingeteilt.".to_string(),
                ));
            }
            Err(e)
                if e.as_database_error()
                    .is_some_and(|e| e.is_foreign_key_violation()) =>
            {
                return Err(RegistrationError::NotFound);
            }
            Err(e) => return Err(e.into()),
        }
        tx.commit().await?;
        Ok(())
    }

    async fn unassign_judge(&self, judge_id: JudgeId, unit_id: JuryUnitId) -> Result<(), DbError> {
        let mut tx = self.begin_write().await?;
        sqlx::query!(
            "DELETE FROM jury_assignments WHERE judge_id = $1 AND unit_id = $2",
            judge_id,
            unit_id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn edit_jury_unit(&self, fields: &JuryUnitFields) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let result = match fields.kind {
            JuryUnitKind::Heat => {
                sqlx::query!(
                    r#"
                    UPDATE heats SET
                        jury_registrable = $1, jury_deployments = $2,
                        jury_needed_technical = $3, jury_needed_performance = $4,
                        jury_needed_dismounts = $5
                    WHERE id = $6
                    "#,
                    fields.registrable,
                    fields.deployments,
                    fields.needed_technical,
                    fields.needed_performance,
                    fields.needed_dismounts,
                    fields.id,
                )
                .execute(&mut *tx)
                .await?
            }
            JuryUnitKind::Final => {
                sqlx::query!(
                    r#"
                    UPDATE finals SET
                        jury_registrable = $1, jury_deployments = $2,
                        jury_needed_technical = $3, jury_needed_performance = $4,
                        jury_needed_dismounts = $5
                    WHERE id = $6
                    "#,
                    fields.registrable,
                    fields.deployments,
                    fields.needed_technical,
                    fields.needed_performance,
                    fields.needed_dismounts,
                    fields.id,
                )
                .execute(&mut *tx)
                .await?
            }
        };
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn move_judge_registrations(
        &self,
        judge_id: JudgeId,
        from_id: JuryUnitId,
        to_kind: JuryUnitKind,
        to_id: JuryUnitId,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        match unit_is_registrable(&mut tx, to_kind, to_id).await? {
            None => return Err(RegistrationError::NotFound),
            Some(false) if from_id != to_id => return Err(not_registrable()),
            Some(_) => {}
        }
        if from_id == to_id {
            return Ok(());
        }
        let (to_heat, to_final) = unit_columns(to_kind, to_id);
        sqlx::query!(
            r#"
            INSERT INTO judge_registration (judge_id, heat_id, final_id, task)
            SELECT judge_id, $1, $2, task FROM judge_registration
            WHERE judge_id = $3 AND unit_id = $4
            ON CONFLICT DO NOTHING
            "#,
            to_heat,
            to_final,
            judge_id,
            from_id,
        )
        .execute(&mut *tx)
        .await?;
        let result = sqlx::query!(
            "DELETE FROM judge_registration WHERE judge_id = $1 AND unit_id = $2",
            judge_id,
            from_id,
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn resolve_unmapped_registration(
        &self,
        id: i64,
        unit: Option<(JuryUnitKind, JuryUnitId)>,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let unmapped = sqlx::query!(
            r#"
            SELECT judge_id as "judge_id!: JudgeId", task as "task!: JudgeTask"
            FROM judge_registration_unmapped WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RegistrationError::NotFound)?;
        if let Some((kind, unit_id)) = unit {
            match unit_is_registrable(&mut tx, kind, unit_id).await? {
                None => return Err(RegistrationError::NotFound),
                Some(false) => return Err(not_registrable()),
                Some(true) => {}
            }
            let (heat_id, final_id) = unit_columns(kind, unit_id);
            sqlx::query!(
                "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                unmapped.judge_id,
                heat_id,
                final_id,
                unmapped.task,
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!("DELETE FROM judge_registration_unmapped WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn apply_jury_remap(
        &self,
        registrations: Vec<RemapRegistration>,
    ) -> Result<RemapOutcome, DbError> {
        let mut tx = self.begin_write().await?;
        let mut registered = 0;
        let mut unfilled = Vec::new();
        let mut not_found = Vec::new();
        for registration in registrations {
            let label = registration.jury_unit.trim();
            if label.is_empty() {
                unfilled.push(registration);
                continue;
            }
            let judges = sqlx::query_scalar!(
                r#"
                SELECT judge.id as "id!: JudgeId" FROM judge
                JOIN clubs ON clubs.id = judge.club_id
                WHERE clubs.name = $1 AND judge.firstname = $2 AND judge.lastname = $3
                "#,
                registration.club,
                registration.firstname,
                registration.lastname,
            )
            .fetch_all(&mut *tx)
            .await?;
            let [judge] = judges[..] else {
                not_found.push((registration, RemapProblem::JudgeNotFound));
                continue;
            };
            let unit = match parse_unit_label(label) {
                Some((kind, name)) => sqlx::query!(
                    r#"SELECT id as "id!: JuryUnitId" FROM view_jury_unit WHERE kind = $1 AND name = $2"#,
                    kind,
                    name
                )
                .fetch_optional(&mut *tx)
                .await?
                .map(|row| (kind, row.id)),
                None => None,
            };
            let Some((kind, unit)) = unit else {
                not_found.push((registration, RemapProblem::JuryUnitNotFound));
                continue;
            };
            if unit_is_registrable(&mut tx, kind, unit).await? != Some(true) {
                not_found.push((registration, RemapProblem::JuryUnitNotRegistrable));
                continue;
            }
            let (heat_id, final_id) = unit_columns(kind, unit);
            sqlx::query!(
                r#"
                INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)
                ON CONFLICT DO NOTHING
                "#,
                judge,
                heat_id,
                final_id,
                registration.task,
            )
            .execute(&mut *tx)
            .await?;
            registered += 1;
        }
        tx.commit().await?;
        Ok(RemapOutcome {
            registered,
            unfilled,
            not_found,
        })
    }

    async fn set_jury_deployments_override(
        &self,
        club_id: ClubId,
        deployments: Option<i64>,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query!(
            "UPDATE clubs SET jury_deployments_override = $1 WHERE id = $2",
            deployments,
            club_id
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn set_jury_plan_published(
        &self,
        published: bool,
        now: OffsetDateTime,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        if published && let Some(competition) = crate::competition::current(&mut *tx).await? {
            // The jury's own deadline only counts with the jury rules.
            let end = competition.judge_registration_end();
            if now <= end {
                return Err(RegistrationError::Rejected(format!(
                    "Der Juryplan kann erst nach dem Ende der Jury-Anmeldung ({:02}.{:02}.{}) veröffentlicht werden.",
                    end.day(),
                    u8::from(end.month()),
                    end.year()
                )));
            }
        }
        let published_at = published.then_some(now);
        sqlx::query!(
            "UPDATE competition SET jury_plan_published_at = $1",
            published_at
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
