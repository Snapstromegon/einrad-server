//! `JuryCommands` for SQLite: judges, their registrations and the Juryplan
//! (#225).

use std::collections::HashSet;

use db_core::{
    actor::Actor,
    clubs::RegistrationError,
    commands::jury::{
        AddClubJudge, ApplyJuryRemap, AssignJudge, DeleteClubJudge, EditClubJudge, EditJuryUnit,
        JuryCommands, MoveJudgeRegistrations, ResolveUnmappedRegistration,
        SetJuryDeploymentsOverride, SetJuryPlanPublished, UnassignJudge,
    },
    error::CommandError,
    ids::{ClubId, JudgeId, JuryUnitId},
    jury::{
        self, JudgeAgeRule, JudgeFields, JudgeTask, JuryUnitFields, JuryUnitKind, RemapOutcome,
        RemapProblem, RemapRegistration, already_assigned_message, check_deployments_override,
        check_plan_publishable, check_task_assignable, check_unit_registration, parse_unit_label,
        unit_columns, unit_registrable,
    },
    registration_rules::RiderName,
};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

use super::{SqliteRepo, write_tx};
use crate::registration::{club_access, judge_access, map_duplicate};

/// Whether judges can register for the heat or final now; `None` for an
/// unknown unit.
async fn unit_is_registrable(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    id: JuryUnitId,
) -> sqlx::Result<Option<bool>> {
    Ok(sqlx::query!(
        r#"
        SELECT
            registrable_setting as "registrable_setting!: bool",
            skipped as "skipped!: bool"
        FROM view_jury_unit WHERE kind = $1 AND id = $2
        "#,
        kind,
        id
    )
    .fetch_optional(conn)
    .await?
    .map(|row| unit_registrable(row.registrable_setting, row.skipped)))
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
    let names: Vec<RiderName> = names
        .into_iter()
        .map(|row| RiderName {
            firstname: row.firstname,
            lastname: row.lastname,
        })
        .collect();
    jury::ensure_judge_name_is_free(&names, firstname, lastname)
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
    // Untrained people only count dismounts, also in the Juryplan.
    for task in JudgeTask::ALL
        .into_iter()
        .filter(|task| !task.assignable_to(fields.is_trained))
    {
        sqlx::query!(
            "DELETE FROM jury_assignments WHERE judge_id = $1 AND task = $2",
            judge_id,
            task
        )
        .execute(&mut *conn)
        .await?;
    }
    for registration in &fields.registrations {
        let registrable =
            unit_is_registrable(conn, registration.unit_kind, registration.unit_id).await?;
        // Only what the judge already has there may stay, per task.
        let already_registered = existing.contains(&(
            registration.unit_kind,
            registration.unit_id,
            registration.task,
        ));
        check_unit_registration(registrable, already_registered)?;
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
impl JuryCommands for SqliteRepo {
    async fn add_club_judge(
        &self,
        actor: &Actor,
        cmd: AddClubJudge,
    ) -> Result<JudgeId, CommandError> {
        Ok(write_tx!(self, tx, {
            add_club_judge(&mut tx, actor, cmd.club_id, cmd.fields).await?
        }))
    }

    async fn edit_club_judge(&self, actor: &Actor, cmd: EditClubJudge) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            edit_club_judge(&mut tx, actor, cmd.judge_id, cmd.fields).await?
        });
        Ok(())
    }

    async fn delete_club_judge(
        &self,
        actor: &Actor,
        cmd: DeleteClubJudge,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            delete_club_judge(&mut tx, actor, cmd.judge_id).await?
        });
        Ok(())
    }

    async fn edit_jury_unit(&self, _actor: &Actor, cmd: EditJuryUnit) -> Result<(), CommandError> {
        cmd.fields.validate().map_err(CommandError::Rejected)?;
        write_tx!(self, tx, { edit_jury_unit(&mut tx, &cmd.fields).await? });
        Ok(())
    }

    async fn assign_judge(&self, _actor: &Actor, cmd: AssignJudge) -> Result<(), CommandError> {
        write_tx!(self, tx, { assign_judge(&mut tx, &cmd).await? });
        Ok(())
    }

    async fn unassign_judge(&self, _actor: &Actor, cmd: UnassignJudge) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            unassign_judge(&mut tx, cmd.judge_id, cmd.unit_id).await?
        });
        Ok(())
    }

    async fn move_judge_registrations(
        &self,
        _actor: &Actor,
        cmd: MoveJudgeRegistrations,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { move_judge_registrations(&mut tx, cmd).await? });
        Ok(())
    }

    async fn resolve_unmapped_registration(
        &self,
        _actor: &Actor,
        cmd: ResolveUnmappedRegistration,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            resolve_unmapped_registration(&mut tx, cmd.id, cmd.unit).await?
        });
        Ok(())
    }

    async fn apply_jury_remap(
        &self,
        _actor: &Actor,
        cmd: ApplyJuryRemap,
    ) -> Result<RemapOutcome, CommandError> {
        Ok(write_tx!(self, tx, {
            apply_jury_remap(&mut tx, cmd.registrations).await?
        }))
    }

    async fn set_jury_deployments_override(
        &self,
        _actor: &Actor,
        cmd: SetJuryDeploymentsOverride,
    ) -> Result<(), CommandError> {
        check_deployments_override(cmd.deployments)?;
        write_tx!(self, tx, {
            set_jury_deployments_override(&mut tx, cmd.club_id, cmd.deployments).await?
        });
        Ok(())
    }

    async fn set_jury_plan_published(
        &self,
        _actor: &Actor,
        cmd: SetJuryPlanPublished,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_jury_plan_published(&mut tx, cmd.published, cmd.now).await?
        });
        Ok(())
    }
}

async fn add_club_judge(
    conn: &mut SqliteConnection,
    actor: &Actor,
    club_id: ClubId,
    fields: JudgeFields,
) -> Result<JudgeId, CommandError> {
    club_access(conn, actor, club_id).await?;
    let rule = judge_age_rule(conn).await?;
    let fields = fields
        .normalize(rule)
        .map_err(RegistrationError::Rejected)?;
    let judge_id = JudgeId::new();
    save_judge(conn, judge_id, club_id, &fields).await?;
    Ok(judge_id)
}

async fn edit_club_judge(
    conn: &mut SqliteConnection,
    actor: &Actor,
    judge_id: JudgeId,
    fields: JudgeFields,
) -> Result<(), CommandError> {
    let club_id = judge_access(conn, actor, judge_id).await?;
    let rule = judge_age_rule(conn).await?;
    let fields = fields
        .normalize(rule)
        .map_err(RegistrationError::Rejected)?;
    save_judge(conn, judge_id, club_id, &fields).await
}

async fn delete_club_judge(
    conn: &mut SqliteConnection,
    actor: &Actor,
    judge_id: JudgeId,
) -> Result<(), CommandError> {
    judge_access(conn, actor, judge_id).await?;
    // The registrations go with it (foreign key).
    sqlx::query!("DELETE FROM judge WHERE id = $1", judge_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn unassign_judge(
    conn: &mut SqliteConnection,
    judge_id: JudgeId,
    unit_id: JuryUnitId,
) -> Result<(), CommandError> {
    sqlx::query!(
        "DELETE FROM jury_assignments WHERE judge_id = $1 AND unit_id = $2",
        judge_id,
        unit_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn assign_judge(
    conn: &mut SqliteConnection,
    assignment: &AssignJudge,
) -> Result<(), CommandError> {
    let is_trained = sqlx::query_scalar!(
        "SELECT is_trained FROM judge WHERE id = $1",
        assignment.judge_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    check_task_assignable(is_trained, assignment.task)?;
    let (heat_id, final_id) = unit_columns(assignment.unit_kind, assignment.unit_id);
    let result = sqlx::query!(
        "INSERT INTO jury_assignments (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)",
        assignment.judge_id,
        heat_id,
        final_id,
        assignment.task,
    )
    .execute(&mut *conn)
    .await;
    match result {
        Ok(_) => {}
        Err(e)
            if e.as_database_error()
                .is_some_and(|e| e.is_unique_violation()) =>
        {
            // A unique constraint: only the database detects it.
            return Err(RegistrationError::Rejected(already_assigned_message()));
        }
        Err(e)
            if e.as_database_error()
                .is_some_and(|e| e.is_foreign_key_violation()) =>
        {
            return Err(RegistrationError::NotFound);
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

async fn edit_jury_unit(
    conn: &mut SqliteConnection,
    fields: &JuryUnitFields,
) -> Result<(), CommandError> {
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
            .execute(&mut *conn)
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
            .execute(&mut *conn)
            .await?
        }
    };
    if result.rows_affected() == 0 {
        return Err(RegistrationError::NotFound);
    }
    Ok(())
}

async fn move_judge_registrations(
    conn: &mut SqliteConnection,
    cmd: MoveJudgeRegistrations,
) -> Result<(), CommandError> {
    let MoveJudgeRegistrations {
        judge_id,
        from_id,
        to_kind,
        to_id,
    } = cmd;
    match unit_is_registrable(conn, to_kind, to_id).await? {
        None => return Err(RegistrationError::NotFound),
        // Moving within a unit changes nothing.
        registrable => check_unit_registration(registrable, from_id == to_id)?,
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
    .execute(&mut *conn)
    .await?;
    let result = sqlx::query!(
        "DELETE FROM judge_registration WHERE judge_id = $1 AND unit_id = $2",
        judge_id,
        from_id,
    )
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(RegistrationError::NotFound);
    }
    Ok(())
}

async fn resolve_unmapped_registration(
    conn: &mut SqliteConnection,
    id: i64,
    unit: Option<(JuryUnitKind, JuryUnitId)>,
) -> Result<(), CommandError> {
    let unmapped = sqlx::query!(
        r#"
        SELECT judge_id as "judge_id!: JudgeId", task as "task!: JudgeTask"
        FROM judge_registration_unmapped WHERE id = $1
        "#,
        id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    if let Some((kind, unit_id)) = unit {
        match unit_is_registrable(conn, kind, unit_id).await? {
            None => return Err(RegistrationError::NotFound),
            registrable => check_unit_registration(registrable, false)?,
        }
        let (heat_id, final_id) = unit_columns(kind, unit_id);
        sqlx::query!(
            "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            unmapped.judge_id,
            heat_id,
            final_id,
            unmapped.task,
        )
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query!("DELETE FROM judge_registration_unmapped WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn apply_jury_remap(
    conn: &mut SqliteConnection,
    registrations: Vec<RemapRegistration>,
) -> Result<RemapOutcome, CommandError> {
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
        .fetch_all(&mut *conn)
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
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| (kind, row.id)),
            None => None,
        };
        let Some((kind, unit)) = unit else {
            not_found.push((registration, RemapProblem::JuryUnitNotFound));
            continue;
        };
        if unit_is_registrable(conn, kind, unit).await? != Some(true) {
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
        .execute(&mut *conn)
        .await?;
        registered += 1;
    }
    Ok(RemapOutcome {
        registered,
        unfilled,
        not_found,
    })
}

async fn set_jury_deployments_override(
    conn: &mut SqliteConnection,
    club_id: ClubId,
    deployments: Option<i64>,
) -> Result<(), CommandError> {
    let result = sqlx::query!(
        "UPDATE clubs SET jury_deployments_override = $1 WHERE id = $2",
        deployments,
        club_id
    )
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(RegistrationError::NotFound);
    }
    Ok(())
}

async fn set_jury_plan_published(
    conn: &mut SqliteConnection,
    published: bool,
    now: OffsetDateTime,
) -> Result<(), CommandError> {
    if let Some(competition) = crate::competition::current(&mut *conn).await? {
        // The jury's own deadline only counts with the jury rules.
        check_plan_publishable(published, competition.judge_registration_end(), now)?;
    }
    let published_at = published.then_some(now);
    sqlx::query!(
        "UPDATE competition SET jury_plan_published_at = $1",
        published_at
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}
