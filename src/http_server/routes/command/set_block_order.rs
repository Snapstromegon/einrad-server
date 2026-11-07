use std::{collections::HashSet, sync::Arc};

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{EntryKind, Slot, load},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetBlockOrderResponse {}

/// One slot of a start block.
#[derive(Debug, Deserialize, ToSchema)]
pub enum BlockSlotInput {
    /// An act, from this block or any other (if it hasn't started).
    Act { act_id: Uuid },
    /// A break. With `id`: an existing break of this block, whose label and
    /// duration change, also while it runs. Without: a new break.
    Break {
        id: Option<i64>,
        label: String,
        duration_seconds: i64,
    },
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBlockOrderBody {
    timeplan_id: i64,
    /// The complete order of the block. Its started and ended slots must
    /// come first, as they are.
    slots: Vec<BlockSlotInput>,
}

fn invalid(message: &str) -> HttpError {
    HttpError::ErrorMessages(message.to_string())
}

/// Sets the order of a start block: which acts (of any category) and breaks
/// run in it, and in which order.
///
/// Acts from other blocks leave them. Acts of this block that the list leaves
/// out go back to their home block (the first block of their category), to
/// its end; breaks it leaves out are deleted. Only slots that haven't started
/// can be moved, also in the running block. Ended blocks can't be changed.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/set_block_order",
    request_body=SetBlockOrderBody,
    responses(
        (status=200, content_type="application/json", body=SetBlockOrderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn set_block_order(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetBlockOrderBody>,
) -> Result<Json<SetBlockOrderResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let timeplan = load(&mut tx).await?;
    let block = timeplan
        .entries
        .iter()
        .find(|e| e.id == body.timeplan_id)
        .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if block.kind != EntryKind::Acts {
        return Err(invalid("Nur Startblöcke haben eine Reihenfolge."));
    }
    if block.final_name.is_some() {
        return Err(invalid(
            "Ein Finale hat feste Finalplätze; die ändern sich im Finale selbst.",
        ));
    }
    if block.ended_at.is_some() {
        return Err(invalid("Der Block ist schon beendet."));
    }

    // The started slots come first (see `timeplan::load`) and stay.
    let started = block.slots.iter().take_while(|s| s.is_started()).count();
    let kept = body.slots.len() >= started
        && block.slots[..started]
            .iter()
            .zip(&body.slots)
            .all(|(slot, input)| match (slot, input) {
                (Slot::Act(act), BlockSlotInput::Act { act_id }) => act.id == *act_id,
                (Slot::Break(pause), BlockSlotInput::Break { id, .. }) => Some(pause.id) == *id,
                _ => false,
            });
    if !kept {
        return Err(invalid(
            "Gestartete und beendete Einträge bleiben am Anfang des Blocks, wie sie sind.",
        ));
    }

    let own_breaks: HashSet<i64> = block
        .slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Break(pause) => Some(pause.id),
            Slot::Act(_) | Slot::Finalist(_) => None,
        })
        .collect();
    let mut acts = HashSet::new();
    // The started breaks can't come again later.
    let mut breaks: HashSet<i64> = body.slots[..started]
        .iter()
        .filter_map(|input| match input {
            BlockSlotInput::Break { id, .. } => *id,
            BlockSlotInput::Act { .. } => None,
        })
        .collect();
    for input in &body.slots[started..] {
        match input {
            BlockSlotInput::Act { act_id } => {
                let act = timeplan
                    .acts
                    .get(act_id)
                    .ok_or_else(|| invalid("Unbekannte Kür oder Kür ohne Kategorie."))?;
                if act.started_at.is_some() {
                    return Err(invalid(&format!(
                        "„{}“ hat schon begonnen und bleibt, wo sie ist.",
                        act.name
                    )));
                }
                if !acts.insert(*act_id) {
                    return Err(invalid("Eine Kür kann nur einmal im Block stehen."));
                }
            }
            BlockSlotInput::Break { id: Some(id), .. } => {
                if !own_breaks.contains(id) || !breaks.insert(*id) {
                    return Err(invalid("Unbekannte Pause."));
                }
            }
            BlockSlotInput::Break { id: None, .. } => {}
        }
    }
    for input in &body.slots {
        if let BlockSlotInput::Break {
            label,
            duration_seconds,
            ..
        } = input
        {
            if label.trim().is_empty() {
                return Err(invalid("Eine Pause braucht eine Bezeichnung."));
            }
            if *duration_seconds <= 0 {
                return Err(invalid("Eine Pause braucht eine Dauer."));
            }
        }
    }

    // Acts left out go back to their home block, which mustn't be over.
    let left_out = block.slots.iter().filter_map(|slot| match slot {
        Slot::Act(act) if !acts.contains(&act.id) => Some(act),
        _ => None,
    });
    timeplan
        .check_way_home(left_out, None)
        .map_err(HttpError::ErrorMessages)?;

    info!(
        "Setting the order of timeplan block {} to {} slots",
        block.id,
        body.slots.len()
    );
    // Breaks left out are deleted; acts left out go home, since they no
    // longer sit in any block's slots.
    for id in own_breaks.iter().filter(|id| {
        !body.slots.iter().any(
            |input| matches!(input, BlockSlotInput::Break { id: Some(kept), .. } if kept == *id),
        )
    }) {
        sqlx::query!("DELETE FROM timeplan_slots WHERE id = ?", id)
            .execute(&mut *tx)
            .await?;
    }
    // Acts without a category at the moment (withdrawn, incomplete groups)
    // aren't shown, so they aren't sent either: they keep their place.
    for slot in &block.slots {
        if let Slot::Act(act) = slot {
            sqlx::query!(
                "DELETE FROM timeplan_slots WHERE timeplan_id = ? AND act_id = ?",
                block.id,
                act.id
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    for (position, input) in body.slots.iter().enumerate() {
        let position = position as i64;
        match input {
            BlockSlotInput::Act { act_id } => {
                // From any other block.
                sqlx::query!("DELETE FROM timeplan_slots WHERE act_id = ?", act_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query!(
                    "INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES (?, ?, ?)",
                    block.id,
                    position,
                    act_id
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
                    "UPDATE timeplan_slots SET position = ?, label = ?, duration_seconds = ? WHERE id = ?",
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
                    "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) VALUES (?, ?, ?, ?)",
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

    // The next act may be another one now.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetBlockOrderResponse {}))
}
