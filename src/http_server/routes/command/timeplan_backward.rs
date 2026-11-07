use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{Entry, Slot, load, reopen_slot, unstart_slot},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetTimeplanBackwardResponse {}

/// Moves the live timeplan one step back, undoing the last forward step.
///
/// Undoing the start of an entry also undoes the skips of the empty start
/// blocks right before it, since they were skipped in the same step. Undoing
/// the start of a slot that followed a break reopens the break, since ending
/// it started the slot.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/timeplan_backward",
    responses(
        (status=200, content_type="application/json", body=SetTimeplanBackwardResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn timeplan_backward(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ControlMusic>,
) -> Result<Json<SetTimeplanBackwardResponse>, HttpError> {
    let db = db.get().await.clone();
    // One write transaction, so two operators pressing at the same moment
    // can't both act on the same state and skip a step.
    let mut tx = begin_write(&db).await?;

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
                // Nothing done in this entry yet: undo its start, together
                // with the empty blocks skipped to get here.
                sqlx::query!(
                    "UPDATE timeplan SET started_at = NULL WHERE id = ?",
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
            // Reopen the last entry that really ran (and its last slot), and
            // undo the skips after it.
            let Some(index) = entries
                .iter()
                .rposition(|e| e.ended_at.is_some() && !e.is_skipped())
            else {
                tx.commit().await?;
                return Ok(Json(SetTimeplanBackwardResponse {}));
            };
            let entry = &entries[index];
            sqlx::query!("UPDATE timeplan SET ended_at = NULL WHERE id = ?", entry.id)
                .execute(&mut *tx)
                .await?;
            // Its last slot runs again if ending it ended the block; a block
            // that ended because its last acts were moved away just reopens.
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
    // A remote player gets the song of the act now due; it doesn't start.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }

    Ok(Json(SetTimeplanBackwardResponse {}))
}

/// Puts a skipped entry back to planned, with its breaks (a block can lose
/// its acts while one of its breaks runs).
async fn reset_entry(db: &mut sqlx::SqliteConnection, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE timeplan SET started_at = NULL, ended_at = NULL WHERE id = ?",
        id
    )
    .execute(&mut *db)
    .await?;
    sqlx::query!(
        "UPDATE timeplan_slots SET started_at = NULL, ended_at = NULL WHERE timeplan_id = ? AND act_id IS NULL",
        id
    )
    .execute(&mut *db)
    .await?;
    Ok(())
}
