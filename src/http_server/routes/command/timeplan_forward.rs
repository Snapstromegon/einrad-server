use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{Entry, EntryKind, Slot, end_slot, load, now, start_slot},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetTimeplanForwardResponse {}

/// Moves the live timeplan one step forward.
///
/// Ends the running act or break, starts the next slot of the running block,
/// or ends the running entry and starts the next one. Ending a break starts
/// the next slot in the same step; ending the last slot ends the block. Empty
/// start blocks are skipped on the way. Ending an act stops the music on the
/// active player.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/timeplan_forward",
    responses(
        (status=200, content_type="application/json", body=SetTimeplanForwardResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn timeplan_forward(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ControlMusic>,
) -> Result<Json<SetTimeplanForwardResponse>, HttpError> {
    info!("Forwarding timeplan");
    let db = db.get().await.clone();
    // One write transaction, so two operators pressing at the same moment
    // can't both act on the same state and skip a step.
    let mut tx = begin_write(&db).await?;
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
                // they were moved away): the block is over. A block without
                // acts doesn't run its breaks either.
                _ if entry.is_empty() => end_entry(&mut tx, entry.id, &now).await?,
                None => end_entry(&mut tx, entry.id, &now).await?,
                // The act is judged now; the next slot starts with the next
                // step.
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
    // A remote player gets the song of the act now due; it doesn't start.
    // Judging starts in silence.
    let cued = if ended_act {
        player_hub.stop_and_cue_from_timeplan().await
    } else {
        player_hub.cue_from_timeplan().await
    };
    if let Err(e) = cued {
        warn!("Couldn't cue the next song: {e}");
    }

    Ok(Json(SetTimeplanForwardResponse {}))
}

/// Starts the next entry that isn't empty, and skips the empty start blocks
/// before it. Its first slot starts right away if it has no warm-up. If only
/// empty blocks are left, nothing changes.
async fn start_next_timeplan_entry(
    db: &mut sqlx::SqliteConnection,
    entries: &[Entry],
    now: &str,
) -> Result<(), sqlx::Error> {
    let upcoming: Vec<&Entry> = entries.iter().filter(|e| e.started_at.is_none()).collect();
    let Some(next) = upcoming.iter().position(|e| !e.is_empty()) else {
        return Ok(());
    };

    for skipped in &upcoming[..next] {
        sqlx::query!(
            "UPDATE timeplan SET started_at = ?, ended_at = ? WHERE id = ?",
            now,
            now,
            skipped.id
        )
        .execute(&mut *db)
        .await?;
    }

    let next = upcoming[next];
    sqlx::query!(
        "UPDATE timeplan SET started_at = ? WHERE id = ?",
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

async fn end_entry(db: &mut sqlx::SqliteConnection, id: i64, now: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("UPDATE timeplan SET ended_at = ? WHERE id = ?", now, id)
        .execute(db)
        .await?;
    Ok(())
}
