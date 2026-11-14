use std::collections::HashMap;

use db_core::{
    DbError, TimeplanRepo,
    clubs::RegistrationError,
    ids::{ActId, CategoryId, FinalId, HeatId},
    timeplan::{
        Audience, BlockSlotInput, Entry, EntryFields, EntryKind, INTERNAL_PUBLIC_LABEL, Slot,
        Timeplan, TimeplanListEntry, check_block_order, own_breaks,
    },
};
use sqlx::SqliteConnection;
use tracing::info;

use super::SqliteRepo;
use crate::{
    structure::{check_final_plannable, heat_code_of},
    timeplan::{
        end_slot, entry_ids, load, load_for, now, reopen_slot, set_entry_order, start_slot,
        swap_with_neighbour, unstart_slot,
    },
};

#[async_trait::async_trait]
impl TimeplanRepo for SqliteRepo {
    async fn load_timeplan(&self, audience: Audience) -> Result<Timeplan, DbError> {
        let pool = self.pool().await;
        // One read transaction, so the blocks and acts are read consistently.
        let mut tx = pool.begin().await?;
        let loaded = load_for(&mut tx, audience).await?;
        tx.commit().await?;
        Ok(loaded)
    }

    async fn list_timeplan(&self, audience: Audience) -> Result<Vec<TimeplanListEntry>, DbError> {
        let pool = self.pool().await;
        // The public gets internal entries as a "Pause", like in the prediction.
        let public = audience == Audience::Public;
        Ok(sqlx::query_as!(
            TimeplanListEntry,
            r#"
            SELECT
                id,
                kind as "kind: EntryKind",
                earliest_start_time,
                duration_seconds,
                CASE WHEN internal AND ?1 THEN ?2 ELSE label END as "label: String",
                category_id as "category_id: CategoryId",
                (SELECT name FROM categories WHERE categories.id = timeplan.category_id) as category,
                final_id as "final_id: FinalId",
                (SELECT name FROM finals WHERE finals.id = timeplan.final_id) as final_name,
                einfahrzeit_seconds,
                show_photos,
                internal AND NOT ?1 as "internal!: bool",
                EXISTS (
                    SELECT 1 FROM timeplan_slots WHERE timeplan_slots.timeplan_id = timeplan.id
                ) as "manually_ordered!: bool",
                started_at,
                ended_at
            FROM timeplan
            ORDER BY position, id
            "#,
            public,
            INTERNAL_PUBLIC_LABEL,
        )
        .fetch_all(&pool)
        .await?)
    }

    async fn add_timeplan_entry(
        &self,
        fields: &EntryFields,
        earliest_start_time: Option<&str>,
        after_id: Option<i64>,
    ) -> Result<i64, RegistrationError> {
        let mut tx = self.begin_write().await?;
        if let Some(final_id) = fields.final_id {
            check_final_plannable(&mut tx, final_id, None)
                .await
                .map_err(RegistrationError::Rejected)?;
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
        .fetch_one(&mut *tx)
        .await?;

        if let Some(after_id) = after_id {
            let mut ids = entry_ids(&mut tx).await?;
            ids.retain(|&e| e != id);
            let index = ids
                .iter()
                .position(|&e| e == after_id)
                .ok_or(RegistrationError::NotFound)?;
            ids.insert(index + 1, id);
            set_entry_order(&mut tx, &ids).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    async fn edit_timeplan_entry(
        &self,
        id: i64,
        fields: &EntryFields,
        earliest_start_time: Option<&str>,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        if let Some(final_id) = fields.final_id {
            check_final_plannable(&mut tx, final_id, Some(id))
                .await
                .map_err(RegistrationError::Rejected)?;
        }
        // A final's block holds only the final's slots.
        let drops_slots = fields.kind == EntryKind::Custom || fields.final_id.is_some();

        // Acts that leave the block go back to their home blocks, which
        // mustn't be over: all of them when it becomes a custom entry or a
        // final's block, those of its old category when that changes.
        let timeplan = load(&mut tx).await?;
        if let Some(block) = timeplan.entries.iter().find(|e| e.id == id) {
            let leaving = block.slots.iter().filter_map(Slot::act).filter(|act| {
                drops_slots || (fields.category_id != block.category_id && !act.stored)
            });
            timeplan
                .check_way_home(leaving, Some(block.id))
                .map_err(RegistrationError::Rejected)?;
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
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        if drops_slots {
            // Its acts go back to their home blocks.
            sqlx::query!("DELETE FROM timeplan_slots WHERE timeplan_id = $1", id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete_timeplan_entry(&self, id: i64) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;

        // Its acts go back to their home blocks, which mustn't be over.
        let timeplan = load(&mut tx).await?;
        if let Some(block) = timeplan.entries.iter().find(|e| e.id == id) {
            timeplan
                .check_way_home(block.slots.iter().filter_map(Slot::act), Some(block.id))
                .map_err(RegistrationError::Rejected)?;
        }

        let result = sqlx::query!("DELETE FROM timeplan WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn move_timeplan_entry(&self, id: i64, up: bool) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        if !swap_with_neighbour(&mut tx, id, up).await? {
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn set_block_order(
        &self,
        timeplan_id: i64,
        slots: &[BlockSlotInput],
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let timeplan = load(&mut tx).await?;
        // The heat codes for the messages about finalist slots.
        let mut heat_codes: HashMap<HeatId, String> = HashMap::new();
        for input in slots {
            if let BlockSlotInput::Finalist { heat_id, .. } = input
                && !heat_codes.contains_key(heat_id)
                && let Some(code) = heat_code_of(&mut tx, *heat_id).await?
            {
                heat_codes.insert(*heat_id, code);
            }
        }
        check_block_order(&timeplan, timeplan_id, slots, &heat_codes)?;
        let block = timeplan
            .entries
            .iter()
            .find(|e| e.id == timeplan_id)
            .ok_or(RegistrationError::NotFound)?;

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
                .execute(&mut *tx)
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
                    .execute(&mut *tx)
                    .await?;
                }
                Slot::Finalist(finalist) => {
                    sqlx::query!(
                        "DELETE FROM timeplan_slots WHERE timeplan_id = $1 AND heat_id = $2 AND number = $3",
                        block.id,
                        finalist.heat_id,
                        finalist.number
                    )
                    .execute(&mut *tx)
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
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query!(
                        "INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES ($1, $2, $3)",
                        block.id,
                        position,
                        act_id
                    )
                    .execute(&mut *tx)
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
                    .execute(&mut *tx)
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
                    .execute(&mut *tx)
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
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn reset_block_order(&self, timeplan_id: i64) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let timeplan = load(&mut tx).await?;
        let block = timeplan
            .entries
            .iter()
            .find(|e| e.id == timeplan_id)
            .ok_or(RegistrationError::NotFound)?;
        if block.started_at.is_some() {
            return Err(RegistrationError::Rejected(
                "Gestartete Blöcke behalten ihre Reihenfolge.".to_string(),
            ));
        }
        // Acts from other blocks go back to theirs, which mustn't be over.
        timeplan
            .check_way_home(block.slots.iter().filter_map(Slot::act), None)
            .map_err(RegistrationError::Rejected)?;

        info!("Resetting the order of timeplan block {timeplan_id}");
        sqlx::query!(
            "DELETE FROM timeplan_slots WHERE timeplan_id = $1",
            timeplan_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn timeplan_forward(&self) -> Result<bool, DbError> {
        // One write transaction, so two operators pressing at the same moment
        // can't both act on the same state and skip a step.
        let mut tx = self.begin_write().await?;
        let mut ended_act = false;

        let timeplan = load(&mut tx).await?;
        let now = now(&mut tx).await?;
        match timeplan.entries.iter().find(|e| e.is_running()) {
            Some(entry) if entry.kind == EntryKind::Acts => {
                let slots = &entry.slots;
                let upcoming = |from: usize| slots[from..].iter().find(|s| !s.is_started());
                let running = slots.iter().position(Slot::is_running);
                if let Some(index) = running {
                    end_slot(&mut tx, &slots[index], &now).await?;
                    // A finalist is judged like an act.
                    ended_act = slots[index].is_start();
                }
                let next = running.map_or_else(|| upcoming(0), |index| upcoming(index + 1));
                match next {
                    // That was the last slot, or the block lost its acts (e.g.
                    // they were moved away): the block is over. A block
                    // without acts doesn't run its breaks either.
                    _ if entry.is_empty() => end_entry(&mut tx, entry.id, &now).await?,
                    None => end_entry(&mut tx, entry.id, &now).await?,
                    // The act is judged now; the next slot starts with the
                    // next step.
                    Some(_) if ended_act => {}
                    // Nothing to judge after a break, or nothing was running:
                    // the next slot starts.
                    Some(slot) => {
                        info!("Starting next slot");
                        start_slot(&mut tx, slot, &now).await?;
                    }
                }
            }
            Some(entry) => end_entry(&mut tx, entry.id, &now).await?,
            None => {
                info!("Starting Next timeplan entry");
                start_next_timeplan_entry(&mut tx, &timeplan.entries, &now).await?;
            }
        }

        tx.commit().await?;
        Ok(ended_act)
    }

    async fn timeplan_backward(&self) -> Result<(), DbError> {
        // One write transaction, so two operators pressing at the same moment
        // can't both act on the same state and skip a step.
        let mut tx = self.begin_write().await?;

        let entries = load(&mut tx).await?.entries;
        match entries.iter().position(Entry::is_running) {
            Some(index) => {
                let entry = &entries[index];
                let slots = &entry.slots;
                if let Some(running) = slots.iter().position(Slot::is_running) {
                    unstart_slot(&mut tx, &slots[running]).await?;
                    // Ending a break started this slot in the same step.
                    if let Some(pause @ Slot::Break(_)) = running.checked_sub(1).map(|i| &slots[i])
                        && pause.ended_at() == slots[running].started_at()
                    {
                        reopen_slot(&mut tx, pause).await?;
                    }
                } else if let Some(last) = slots.iter().rfind(|s| s.is_ended()) {
                    reopen_slot(&mut tx, last).await?;
                } else {
                    // Nothing done in this entry yet: undo its start,
                    // together with the empty blocks skipped to get here.
                    sqlx::query!(
                        "UPDATE timeplan SET started_at = NULL WHERE id = $1",
                        entry.id
                    )
                    .execute(&mut *tx)
                    .await?;
                    let skipped = entries[..index].iter().rev().take_while(|e| e.is_skipped());
                    for skipped in skipped {
                        reset_entry(&mut tx, skipped.id).await?;
                    }
                }
            }
            None => {
                // Reopen the last entry that really ran (and its last slot),
                // and undo the skips after it.
                let Some(index) = entries
                    .iter()
                    .rposition(|e| e.ended_at.is_some() && !e.is_skipped())
                else {
                    tx.commit().await?;
                    return Ok(());
                };
                let entry = &entries[index];
                sqlx::query!(
                    "UPDATE timeplan SET ended_at = NULL WHERE id = $1",
                    entry.id
                )
                .execute(&mut *tx)
                .await?;
                // Its last slot runs again if ending it ended the block; a
                // block that ended because its last acts were moved away just
                // reopens.
                if let Some(last) = entry
                    .slots
                    .iter()
                    .rfind(|s| s.is_ended())
                    .filter(|last| last.ended_at() == entry.ended_at)
                {
                    reopen_slot(&mut tx, last).await?;
                }
                for skipped in entries[index + 1..].iter().filter(|e| e.is_skipped()) {
                    reset_entry(&mut tx, skipped.id).await?;
                }
            }
        }

        tx.commit().await?;
        Ok(())
    }

    async fn mark_music_started(&self, act_id: ActId, seconds_ago: f64) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        let act = sqlx::query!(
            r#"
            UPDATE acts SET music_started_at = datetime(julianday('now') - $1 / 86400.0)
            WHERE id = $2 AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
            "#,
            seconds_ago,
            act_id
        )
        .execute(&mut *tx)
        .await?;
        let finalist = sqlx::query!(
            r#"
            UPDATE final_slots SET music_started_at = datetime(julianday('now') - $1 / 86400.0)
            WHERE act_id = $2 AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
                AND final_id IN (SELECT id FROM finals WHERE placement_published)
            "#,
            seconds_ago,
            act_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(act.rows_affected() + finalist.rows_affected() > 0)
    }
}

/// Starts the next entry that isn't empty, and skips the empty start blocks
/// before it. Its first slot starts right away if it has no warm-up. If only
/// empty blocks are left, nothing changes.
async fn start_next_timeplan_entry(
    db: &mut SqliteConnection,
    entries: &[Entry],
    now: &str,
) -> Result<(), sqlx::Error> {
    let upcoming: Vec<&Entry> = entries.iter().filter(|e| e.started_at.is_none()).collect();
    let Some(next) = upcoming.iter().position(|e| !e.is_empty()) else {
        return Ok(());
    };

    for skipped in &upcoming[..next] {
        sqlx::query!(
            "UPDATE timeplan SET started_at = $1, ended_at = $2 WHERE id = $3",
            now,
            now,
            skipped.id
        )
        .execute(&mut *db)
        .await?;
    }

    let next = upcoming[next];
    sqlx::query!(
        "UPDATE timeplan SET started_at = $1 WHERE id = $2",
        now,
        next.id
    )
    .execute(&mut *db)
    .await?;

    if next.kind == EntryKind::Acts
        && next.warmup_seconds == 0
        && let Some(slot) = next.slots.iter().find(|s| !s.is_started())
    {
        start_slot(db, slot, now).await?;
    }

    Ok(())
}

async fn end_entry(db: &mut SqliteConnection, id: i64, now: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("UPDATE timeplan SET ended_at = $1 WHERE id = $2", now, id)
        .execute(db)
        .await?;
    Ok(())
}

/// Puts a skipped entry back to planned, with its breaks (a block can lose
/// its acts while one of its breaks runs).
async fn reset_entry(db: &mut SqliteConnection, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE timeplan SET started_at = NULL, ended_at = NULL WHERE id = $1",
        id
    )
    .execute(&mut *db)
    .await?;
    sqlx::query!(
        "UPDATE timeplan_slots SET started_at = NULL, ended_at = NULL WHERE timeplan_id = $1 AND act_id IS NULL",
        id
    )
    .execute(&mut *db)
    .await?;
    Ok(())
}
