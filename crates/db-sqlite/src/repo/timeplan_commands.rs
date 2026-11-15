//! `TimeplanCommands` for SQLite.

use std::collections::HashMap;

use db_core::{
    actor::Actor,
    commands::timeplan::{
        AddTimeplanEntry, BlockSlotInput, DeleteTimeplanEntry, EditTimeplanEntry, MoveTimeplanDown,
        MoveTimeplanUp, ResetBlockOrder, SetActMusicStarted, SetBlockOrder, SetStartlistPublished,
        TimeplanCommands,
    },
    error::CommandError,
    ids::HeatId,
    timeplan::{
        Direction, EntryFields, Slot, check_block_order, drops_slots, moved_order, next_step,
        order_after, own_breaks, started_block_keeps_order,
    },
};
use sqlx::SqliteConnection;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::info;

use super::{SqliteRepo, timeplan::mark_music_started_in, write_tx};
use crate::{
    structure::{check_final_plannable, heat_code_of},
    timeplan::{apply_step, entry_ids, load, now, set_entry_order},
};

/// The text a start time is stored as: RFC 3339, as the admin frontend
/// sends it.
fn sqlite_start_time(time: OffsetDateTime) -> Result<String, sqlx::Error> {
    time.format(&Rfc3339)
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

#[async_trait::async_trait]
impl TimeplanCommands for SqliteRepo {
    async fn add_timeplan_entry(
        &self,
        _actor: &Actor,
        cmd: AddTimeplanEntry,
    ) -> Result<i64, CommandError> {
        let earliest_start_time = cmd.earliest_start_time.map(sqlite_start_time).transpose()?;
        Ok(write_tx!(self, tx, {
            add_timeplan_entry(&mut tx, &cmd.fields, earliest_start_time, cmd.after_id).await?
        }))
    }

    async fn edit_timeplan_entry(
        &self,
        _actor: &Actor,
        cmd: EditTimeplanEntry,
    ) -> Result<(), CommandError> {
        let earliest_start_time = cmd.earliest_start_time.map(sqlite_start_time).transpose()?;
        write_tx!(self, tx, {
            edit_timeplan_entry(&mut tx, cmd.id, &cmd.fields, earliest_start_time).await?
        });
        Ok(())
    }

    async fn delete_timeplan_entry(
        &self,
        _actor: &Actor,
        cmd: DeleteTimeplanEntry,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { delete_timeplan_entry(&mut tx, cmd.id).await? });
        Ok(())
    }

    async fn move_timeplan_up(
        &self,
        _actor: &Actor,
        cmd: MoveTimeplanUp,
    ) -> Result<bool, CommandError> {
        self.move_timeplan_entry(cmd.id, true).await
    }

    async fn move_timeplan_down(
        &self,
        _actor: &Actor,
        cmd: MoveTimeplanDown,
    ) -> Result<bool, CommandError> {
        self.move_timeplan_entry(cmd.id, false).await
    }

    async fn set_block_order(
        &self,
        _actor: &Actor,
        cmd: SetBlockOrder,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_block_order(&mut tx, cmd.timeplan_id, &cmd.slots).await?
        });
        Ok(())
    }

    async fn reset_block_order(
        &self,
        _actor: &Actor,
        cmd: ResetBlockOrder,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            reset_block_order(&mut tx, cmd.timeplan_id).await?
        });
        Ok(())
    }

    async fn timeplan_forward(&self, _actor: &Actor) -> Result<bool, CommandError> {
        // One write transaction, so two operators pressing at the same moment
        // can't both act on the same state and skip a step.
        Ok(write_tx!(self, tx, { timeplan_forward(&mut tx).await? }))
    }

    async fn timeplan_backward(&self, _actor: &Actor) -> Result<(), CommandError> {
        write_tx!(self, tx, { timeplan_backward(&mut tx).await? });
        Ok(())
    }

    async fn set_act_music_started(
        &self,
        _actor: &Actor,
        cmd: SetActMusicStarted,
    ) -> Result<bool, CommandError> {
        Ok(write_tx!(self, tx, {
            mark_music_started_in(&mut tx, cmd.act_id, 0.0).await?
        }))
    }

    async fn set_startlist_published(
        &self,
        _actor: &Actor,
        cmd: SetStartlistPublished,
    ) -> Result<bool, CommandError> {
        let mut tx = self.begin_write().await?;
        let published_at = cmd.published.then_some(cmd.at);
        let result = sqlx::query!(
            "UPDATE competition SET startlist_published_at = $1",
            published_at
        )
        .execute(&mut *tx)
        .await?;
        // Without a competition nothing changed, so nothing commits.
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }
}

impl SqliteRepo {
    /// Swaps an entry with its neighbour. Commits only when it moved.
    async fn move_timeplan_entry(&self, id: i64, up: bool) -> Result<bool, CommandError> {
        let mut tx = self.begin_write().await?;
        let Some(ids) = moved_order(&entry_ids(&mut tx).await?, id, up) else {
            return Ok(false);
        };
        set_entry_order(&mut tx, &ids).await?;
        tx.commit().await?;
        Ok(true)
    }
}

async fn add_timeplan_entry(
    conn: &mut SqliteConnection,
    fields: &EntryFields,
    earliest_start_time: Option<String>,
    after_id: Option<i64>,
) -> Result<i64, CommandError> {
    if let Some(final_id) = fields.final_id {
        check_final_plannable(&mut *conn, final_id, None).await?;
    }

    let id = sqlx::query_scalar!(
            r#"
            INSERT INTO timeplan (
                earliest_start_time,
                duration_seconds,
                label,
                category_id,
                final_id,
                kind,
                einfahrzeit_seconds,
                show_photos,
                internal,
                position
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, (SELECT COALESCE(MAX(position), 0) + 1 FROM timeplan))
            RETURNING id as "id!"
            "#,
            earliest_start_time,
            fields.duration_seconds,
            fields.label,
            fields.category_id,
            fields.final_id,
            fields.kind,
            fields.einfahrzeit_seconds,
            fields.show_photos,
            fields.internal,
        )
        .fetch_one(&mut *conn)
        .await?;

    if let Some(after_id) = after_id {
        let ids = order_after(entry_ids(&mut *conn).await?, id, after_id)
            .ok_or(CommandError::NotFound)?;
        set_entry_order(&mut *conn, &ids).await?;
    }
    Ok(id)
}

async fn edit_timeplan_entry(
    conn: &mut SqliteConnection,
    id: i64,
    fields: &EntryFields,
    earliest_start_time: Option<String>,
) -> Result<(), CommandError> {
    if let Some(final_id) = fields.final_id {
        check_final_plannable(&mut *conn, final_id, Some(id)).await?;
    }
    // A final's block holds only the final's slots.
    let drops_slots = drops_slots(fields);

    // Acts that leave the block go back to their home blocks, which
    // mustn't be over: all of them when it becomes a custom entry or a
    // final's block, those of its old category when that changes.
    let timeplan = load(&mut *conn).await?;
    if let Some(block) = timeplan.entries.iter().find(|e| e.id == id) {
        let leaving =
            block.slots.iter().filter_map(Slot::act).filter(|act| {
                drops_slots || (fields.category_id != block.category_id && !act.stored)
            });
        timeplan
            .check_way_home(leaving, Some(block.id))
            .map_err(CommandError::Rejected)?;
    }

    let updated = sqlx::query!(
        r#"
            UPDATE timeplan
            SET
                earliest_start_time = $1,
                duration_seconds = $2,
                label = $3,
                category_id = $4,
                final_id = $5,
                kind = $6,
                einfahrzeit_seconds = $7,
                show_photos = $8,
                internal = $9
            WHERE id = $10
            "#,
        earliest_start_time,
        fields.duration_seconds,
        fields.label,
        fields.category_id,
        fields.final_id,
        fields.kind,
        fields.einfahrzeit_seconds,
        fields.show_photos,
        fields.internal,
        id,
    )
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    if drops_slots {
        // Its acts go back to their home blocks.
        sqlx::query!("DELETE FROM timeplan_slots WHERE timeplan_id = $1", id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

async fn delete_timeplan_entry(conn: &mut SqliteConnection, id: i64) -> Result<(), CommandError> {
    // Its acts go back to their home blocks, which mustn't be over.
    let timeplan = load(&mut *conn).await?;
    if let Some(block) = timeplan.entries.iter().find(|e| e.id == id) {
        timeplan
            .check_way_home(block.slots.iter().filter_map(Slot::act), Some(block.id))
            .map_err(CommandError::Rejected)?;
    }

    let result = sqlx::query!("DELETE FROM timeplan WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    Ok(())
}

async fn set_block_order(
    conn: &mut SqliteConnection,
    timeplan_id: i64,
    slots: &[BlockSlotInput],
) -> Result<(), CommandError> {
    let timeplan = load(&mut *conn).await?;
    // The heat codes for the messages about finalist slots.
    let mut heat_codes: HashMap<HeatId, String> = HashMap::new();
    for input in slots {
        if let BlockSlotInput::Finalist { heat_id, .. } = input
            && !heat_codes.contains_key(heat_id)
            && let Some(code) = heat_code_of(&mut *conn, *heat_id).await?
        {
            heat_codes.insert(*heat_id, code);
        }
    }
    check_block_order(&timeplan, timeplan_id, slots, &heat_codes)?;
    let block = timeplan
        .entries
        .iter()
        .find(|e| e.id == timeplan_id)
        .ok_or(CommandError::NotFound)?;

    info!(
        "Setting the order of timeplan block {} to {} slots",
        block.id,
        slots.len()
    );
    // Breaks left out are deleted; acts left out go home, since they no
    // longer sit in any block's slots.
    for id in own_breaks(block).iter().filter(|id| {
        !slots.iter().any(
            |input| matches!(input, BlockSlotInput::Break { id: Some(kept), .. } if kept == *id),
        )
    }) {
        sqlx::query!("DELETE FROM timeplan_slots WHERE id = $1", id)
            .execute(&mut *conn)
            .await?;
    }
    // Acts without a category at the moment (withdrawn, incomplete
    // groups) aren't shown, so they aren't sent either: they keep their
    // place.
    for slot in &block.slots {
        match slot {
            Slot::Act(act) => {
                sqlx::query!(
                    "DELETE FROM timeplan_slots WHERE timeplan_id = $1 AND act_id = $2",
                    block.id,
                    act.id
                )
                .execute(&mut *conn)
                .await?;
            }
            Slot::Finalist(finalist) => {
                sqlx::query!(
                        "DELETE FROM timeplan_slots WHERE timeplan_id = $1 AND heat_id = $2 AND number = $3",
                        block.id,
                        finalist.heat_id,
                        finalist.number
                    )
                    .execute(&mut *conn)
                    .await?;
            }
            Slot::Break(_) => {}
        }
    }
    for (position, input) in slots.iter().enumerate() {
        let position = position as i64;
        match input {
            BlockSlotInput::Act { act_id } => {
                // From any other block.
                sqlx::query!("DELETE FROM timeplan_slots WHERE act_id = $1", act_id)
                    .execute(&mut *conn)
                    .await?;
                sqlx::query!(
                        "INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES ($1, $2, $3)",
                        block.id,
                        position,
                        act_id
                    )
                    .execute(&mut *conn)
                    .await?;
            }
            BlockSlotInput::Finalist { heat_id, number } => {
                sqlx::query!(
                        "INSERT INTO timeplan_slots (timeplan_id, position, heat_id, number) VALUES ($1, $2, $3, $4)",
                        block.id,
                        position,
                        heat_id,
                        number
                    )
                    .execute(&mut *conn)
                    .await?;
            }
            BlockSlotInput::Break {
                id: Some(id),
                label,
                duration_seconds,
            } => {
                let label = label.trim();
                sqlx::query!(
                        "UPDATE timeplan_slots SET position = $1, label = $2, duration_seconds = $3 WHERE id = $4",
                        position,
                        label,
                        duration_seconds,
                        id
                    )
                    .execute(&mut *conn)
                    .await?;
            }
            BlockSlotInput::Break {
                id: None,
                label,
                duration_seconds,
            } => {
                let label = label.trim();
                sqlx::query!(
                        "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) VALUES ($1, $2, $3, $4)",
                        block.id,
                        position,
                        label,
                        duration_seconds
                    )
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    Ok(())
}

async fn reset_block_order(
    conn: &mut SqliteConnection,
    timeplan_id: i64,
) -> Result<(), CommandError> {
    let timeplan = load(&mut *conn).await?;
    let block = timeplan
        .entries
        .iter()
        .find(|e| e.id == timeplan_id)
        .ok_or(CommandError::NotFound)?;
    if block.started_at.is_some() {
        return Err(CommandError::Rejected(started_block_keeps_order()));
    }
    // Acts from other blocks go back to theirs, which mustn't be over.
    timeplan
        .check_way_home(block.slots.iter().filter_map(Slot::act), None)
        .map_err(CommandError::Rejected)?;

    info!("Resetting the order of timeplan block {timeplan_id}");
    sqlx::query!(
        "DELETE FROM timeplan_slots WHERE timeplan_id = $1",
        timeplan_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn timeplan_forward(conn: &mut SqliteConnection) -> Result<bool, CommandError> {
    let timeplan = load(&mut *conn).await?;
    let now = now(&mut *conn).await?;
    let step = next_step(&timeplan, Direction::Forward);
    apply_step(&mut *conn, &step.ops, &now).await?;
    Ok(step.ended_act)
}

async fn timeplan_backward(conn: &mut SqliteConnection) -> Result<(), CommandError> {
    let timeplan = load(&mut *conn).await?;
    let now = now(&mut *conn).await?;
    let step = next_step(&timeplan, Direction::Backward);
    apply_step(&mut *conn, &step.ops, &now).await?;
    Ok(())
}
