//! `HeatCommands` for SQLite.

use db_core::{
    actor::Actor,
    commands::heat::{AddHeat, DeleteHeat, EditHeat, HeatCommands, SetHeatSkip},
    error::CommandError,
    ids::HeatId,
    structure_rules,
};
use sqlx::SqliteConnection;
use tracing::info;

use super::{SqliteRepo, write_tx};
use crate::structure::{
    category_names, check_heat_categories, check_heat_free, default_threshold_of, finals_of_heat,
    heat_discipline, heat_has_started, heat_name_of, jury_unit_use, set_heat_categories,
};

#[async_trait::async_trait]
impl HeatCommands for SqliteRepo {
    async fn add_heat(&self, _actor: &Actor, cmd: AddHeat) -> Result<(), CommandError> {
        write_tx!(self, tx, { add_heat(&mut tx, cmd).await? });
        Ok(())
    }

    async fn edit_heat(&self, _actor: &Actor, cmd: EditHeat) -> Result<(), CommandError> {
        write_tx!(self, tx, { edit_heat(&mut tx, cmd).await? });
        Ok(())
    }

    async fn delete_heat(&self, _actor: &Actor, cmd: DeleteHeat) -> Result<(), CommandError> {
        write_tx!(self, tx, { delete_heat(&mut tx, cmd.heat_id).await? });
        Ok(())
    }

    async fn set_heat_skip(&self, _actor: &Actor, cmd: SetHeatSkip) -> Result<bool, CommandError> {
        Ok(write_tx!(self, tx, {
            set_heat_skip(&mut tx, cmd.heat_id, cmd.skip).await?
        }))
    }
}

async fn add_heat(conn: &mut SqliteConnection, heat: AddHeat) -> Result<(), CommandError> {
    let categories = category_names(&mut *conn, &heat.category_ids).await?;
    check_heat_free(&mut *conn, None, &heat.name, &heat.code).await?;
    let threshold = match heat.threshold {
        Some(threshold) => threshold,
        None => default_threshold_of(&mut *conn, &heat.category_ids).await?,
    };
    info!("Adding heat {:?}", heat.name);
    let id = HeatId::new();
    sqlx::query!(
        "INSERT INTO heats (id, name, code, description, threshold) VALUES ($1, $2, $3, $4, $5)",
        id,
        heat.name,
        heat.code,
        heat.description,
        threshold
    )
    .execute(&mut *conn)
    .await?;
    // A refusal rolls the new heat back with the transaction.
    check_heat_categories(&mut *conn, id, &categories).await?;
    set_heat_categories(&mut *conn, id, &categories).await?;
    // Groups start in their final: their Vorlauf is skipped from the start
    // (#345); the organizer may restore it.
    if structure_rules::heat_starts_skipped(heat_discipline(&mut *conn, id).await?) {
        sqlx::query!("UPDATE heats SET skip = TRUE WHERE id = $1", id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

async fn edit_heat(conn: &mut SqliteConnection, change: EditHeat) -> Result<(), CommandError> {
    let categories = category_names(&mut *conn, &change.category_ids).await?;
    let Some(name) = heat_name_of(&mut *conn, change.heat_id).await? else {
        return Err(CommandError::NotFound);
    };
    // Without a new code the heat keeps its own.
    let new_code = match &change.new_code {
        Some(code) => code.clone(),
        None => {
            sqlx::query_scalar!(
                r#"SELECT code as "code!" FROM heats WHERE id = $1"#,
                change.heat_id
            )
            .fetch_one(&mut *conn)
            .await?
        }
    };
    check_heat_categories(&mut *conn, change.heat_id, &categories).await?;
    set_heat_categories(&mut *conn, change.heat_id, &categories).await?;
    check_heat_free(
        &mut *conn,
        Some(change.heat_id),
        &change.new_name,
        &new_code,
    )
    .await?;
    if change.new_name != name {
        info!("Renaming heat {name:?} to {:?}", change.new_name);
    }
    sqlx::query!(
            "UPDATE heats SET name = $1, code = $2, description = $3, threshold = coalesce($4, threshold) WHERE id = $5",
            change.new_name,
            new_code,
            change.description,
            change.threshold,
            change.heat_id
        )
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn delete_heat(conn: &mut SqliteConnection, id: HeatId) -> Result<(), CommandError> {
    let finals = finals_of_heat(&mut *conn, id).await?;
    let finals: Vec<String> = finals.into_iter().map(|(name, _)| name).collect();
    let jury_use = jury_unit_use(&mut *conn, "heat", id.into()).await?;
    structure_rules::check_heat_deletable(&finals, jury_use)?;
    info!("Deleting heat {id}");
    // Its categories leave it (foreign key).
    let result = sqlx::query!("DELETE FROM heats WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    Ok(())
}

async fn set_heat_skip(
    conn: &mut SqliteConnection,
    id: HeatId,
    skip: bool,
) -> Result<bool, CommandError> {
    let heat = sqlx::query!(
        r#"SELECT name as "name!", skip FROM heats WHERE id = $1"#,
        id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(CommandError::NotFound)?;
    let started = heat_has_started(&mut *conn, id).await?;
    if !structure_rules::check_set_heat_skip(&heat.name, heat.skip, skip, started)? {
        return Ok(false);
    }
    info!(
        "Setting the Vorlauf of heat {:?} skipped: {skip}",
        heat.name
    );
    sqlx::query!("UPDATE heats SET skip = $1 WHERE id = $2", skip, id)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}
