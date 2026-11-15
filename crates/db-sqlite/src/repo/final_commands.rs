//! `FinalCommands` for SQLite.

use std::collections::HashMap;

use db_core::{
    actor::Actor,
    commands::finals::{
        AddFinal, DeleteFinal, EditFinal, FinalCommands, SetFinalPlacement, SetFinalPublished,
    },
    error::CommandError,
    finals::SlotPlacement,
    ids::{ActId, FinalId, FinalSlotId, HeatId},
    structure_rules::{self, PlacedAct, PlacementSlot},
};
use sqlx::SqliteConnection;
use tracing::info;

use super::{SqliteRepo, write_tx};
use crate::structure::{
    check_final_free, check_final_heats, final_has_started, final_name_of, jury_unit_use,
    replace_final_slots,
};

#[async_trait::async_trait]
impl FinalCommands for SqliteRepo {
    async fn add_final(&self, _actor: &Actor, cmd: AddFinal) -> Result<(), CommandError> {
        write_tx!(self, tx, { add_final(&mut tx, cmd).await? });
        Ok(())
    }

    async fn edit_final(&self, _actor: &Actor, cmd: EditFinal) -> Result<(), CommandError> {
        write_tx!(self, tx, { edit_final(&mut tx, cmd).await? });
        Ok(())
    }

    async fn delete_final(&self, _actor: &Actor, cmd: DeleteFinal) -> Result<(), CommandError> {
        write_tx!(self, tx, { delete_final(&mut tx, cmd.final_id).await? });
        Ok(())
    }

    async fn set_final_placement(
        &self,
        _actor: &Actor,
        cmd: SetFinalPlacement,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_final_placement(&mut tx, cmd.final_id, &cmd.changes).await?
        });
        Ok(())
    }

    async fn set_final_published(
        &self,
        _actor: &Actor,
        cmd: SetFinalPublished,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_final_published(&mut tx, cmd.final_id, cmd.published).await?
        });
        Ok(())
    }
}

async fn add_final(conn: &mut SqliteConnection, new: AddFinal) -> Result<(), CommandError> {
    check_final_free(&mut *conn, None, &new.name, &new.code).await?;
    info!("Adding final {:?}", new.name);
    let fields = &new.fields;
    let id = FinalId::new();
    sqlx::query!(
            r#"
            INSERT INTO finals (
                id, name, code, description, act_type, is_single_male, "order",
                einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
            )
            VALUES ($1, $2, $3, $4, $5, $6, (SELECT COALESCE(MAX("order"), 0) + 1 FROM finals), $7, $8, $9)
            "#,
            id,
            new.name,
            new.code,
            fields.description,
            fields.act_type,
            fields.is_single_male,
            fields.einfahrzeit_seconds,
            fields.act_duration_seconds,
            fields.judge_duration_seconds,
        )
        .execute(&mut *conn)
        .await?;
    check_final_heats(
        &mut *conn,
        id,
        fields.act_type,
        fields.is_single_male,
        &fields.slot_groups,
    )
    .await?;
    replace_final_slots(&mut *conn, id, &fields.slot_groups).await?;
    Ok(())
}

async fn edit_final(conn: &mut SqliteConnection, change: EditFinal) -> Result<(), CommandError> {
    let fields = &change.fields;
    let heats = sqlx::query_scalar!(
            r#"SELECT heat_id as "heat_id!: HeatId" FROM final_slots WHERE final_id = $1 ORDER BY position"#,
            change.final_id
        )
        .fetch_all(&mut *conn)
        .await?;
    let Some(name) = final_name_of(&mut *conn, change.final_id).await? else {
        return Err(CommandError::NotFound);
    };
    // Without a new code the final keeps its own.
    let new_code = match &change.new_code {
        Some(code) => code.clone(),
        None => {
            sqlx::query_scalar!(
                r#"SELECT code as "code!" FROM finals WHERE id = $1"#,
                change.final_id
            )
            .fetch_one(&mut *conn)
            .await?
        }
    };
    check_final_free(
        &mut *conn,
        Some(change.final_id),
        &change.new_name,
        &new_code,
    )
    .await?;
    if change.new_name != name {
        info!("Renaming final {name:?} to {:?}", change.new_name);
    }
    sqlx::query!(
        r#"
            UPDATE finals
            SET
                name = $1,
                code = $2,
                description = $3,
                act_type = $4,
                is_single_male = $5,
                einfahrzeit_seconds = $6,
                act_duration_seconds = $7,
                judge_duration_seconds = $8
            WHERE id = $9
            "#,
        change.new_name,
        new_code,
        fields.description,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
        change.final_id,
    )
    .execute(&mut *conn)
    .await?;
    check_final_heats(
        &mut *conn,
        change.final_id,
        fields.act_type,
        fields.is_single_male,
        &fields.slot_groups,
    )
    .await?;
    let started = final_has_started(&mut *conn, change.final_id).await?;
    if structure_rules::check_slot_groups_change(heats, &fields.slot_groups, started)? {
        info!("Replacing the slots of final {:?}", change.new_name);
        replace_final_slots(&mut *conn, change.final_id, &fields.slot_groups).await?;
    }
    Ok(())
}

async fn delete_final(conn: &mut SqliteConnection, id: FinalId) -> Result<(), CommandError> {
    let started = final_has_started(&mut *conn, id).await?;
    let jury_use = jury_unit_use(&mut *conn, "final", id.into()).await?;
    structure_rules::check_final_deletable(started, jury_use)?;
    info!("Deleting final {id}");
    sqlx::query!("DELETE FROM timeplan WHERE final_id = $1", id)
        .execute(&mut *conn)
        .await?;
    // Its slots go with it (foreign key).
    let result = sqlx::query!("DELETE FROM finals WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    Ok(())
}

async fn set_final_placement(
    conn: &mut SqliteConnection,
    id: FinalId,
    changes: &[SlotPlacement],
) -> Result<(), CommandError> {
    let Some(final_name) = final_name_of(&mut *conn, id).await? else {
        return Err(CommandError::NotFound);
    };
    let slots: Vec<PlacementSlot> = sqlx::query!(
        r#"
            SELECT
                final_slots.id as "id!: FinalSlotId",
                final_slots.heat_id as "heat_id!: HeatId",
                heats.name as "heat_name!",
                heats.code as "heat_code!",
                number,
                act_id as "act_id: ActId",
                final_slots.started_at IS NOT NULL as "started!: bool"
            FROM final_slots
            JOIN heats ON heats.id = final_slots.heat_id
            WHERE final_id = $1
            "#,
        id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| PlacementSlot {
        id: row.id,
        heat_id: row.heat_id,
        heat_name: row.heat_name,
        heat_code: row.heat_code,
        number: row.number,
        act_id: row.act_id,
        started: row.started,
    })
    .collect();
    // The heat of each category.
    let heats: HashMap<String, HeatId> = sqlx::query!(
            r#"SELECT name as "name!", heat_id as "heat_id!: HeatId" FROM categories WHERE heat_id IS NOT NULL"#
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|row| (row.name, row.heat_id))
        .collect();
    // The acts to place: withdrawn acts don't start, they count as
    // without category.
    let mut acts: HashMap<ActId, PlacedAct> = HashMap::new();
    for act_id in changes.iter().filter_map(|change| change.act_id) {
        let category = sqlx::query_scalar!(
                r#"SELECT CAST(CASE WHEN withdrawn_at IS NULL THEN category END AS TEXT) as "category?: String"
                FROM view_act WHERE id = $1"#,
                act_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        acts.insert(
            act_id,
            match category {
                None => PlacedAct::Missing,
                Some(None) => PlacedAct::Unavailable,
                Some(Some(category)) => PlacedAct::InCategory(category),
            },
        );
    }
    let changed = structure_rules::plan_placement(&slots, changes, &acts, &heats)?;
    info!(
        "Placing acts in {} slots of final {:?}",
        changed.len(),
        final_name
    );
    // Empty the changed slots first, so acts can swap slots without
    // clashing in the unique index.
    for (slot_id, _) in &changed {
        sqlx::query!(
            "UPDATE final_slots SET act_id = NULL WHERE id = $1",
            slot_id
        )
        .execute(&mut *conn)
        .await?;
    }
    for (slot_id, act_id) in &changed {
        sqlx::query!(
            "UPDATE final_slots SET act_id = $1 WHERE id = $2",
            act_id,
            slot_id
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn set_final_published(
    conn: &mut SqliteConnection,
    id: FinalId,
    published: bool,
) -> Result<(), CommandError> {
    let started = final_has_started(&mut *conn, id).await?;
    structure_rules::check_final_published(published, started)?;
    info!("Setting the placement of final {id} published: {published}");
    let result = sqlx::query!(
        "UPDATE finals SET placement_published = $1 WHERE id = $2",
        published,
        id
    )
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    Ok(())
}
