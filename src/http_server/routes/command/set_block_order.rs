use crate::ids::{ActId, HeatId};
use std::{collections::HashSet, sync::Arc};

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

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
    /// An act, from this block or any other (if it hasn't started). In a
    /// final's block: an act of a heat whose Vorlauf is skipped, which
    /// starts directly in this final.
    Act { act_id: ActId },
    /// A final's block only: one of its finalist slots, by heat and number
    /// ("2. Finalist (E EM 1)").
    Finalist { heat_id: HeatId, number: i64 },
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
///
/// A final's block orders its finalist slots and the acts of its skipped
/// heats the same way; it takes no other acts, and they go to no other
/// block. Slots and acts it leaves out follow at its end.
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
    let final_id = block.final_id;
    let final_name = block.final_name.as_deref();
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
                (Slot::Finalist(finalist), BlockSlotInput::Finalist { heat_id, number }) => {
                    finalist.heat_id == *heat_id && finalist.number == *number
                }
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
            BlockSlotInput::Act { .. } | BlockSlotInput::Finalist { .. } => None,
        })
        .collect();
    let mut finalists = HashSet::new();
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
                match (final_id, timeplan.direct_acts.get(act_id)) {
                    (Some(id), Some(direct)) if id == *direct => {}
                    (Some(_), _) => {
                        return Err(invalid(&format!(
                            "„{}“ startet nicht direkt im Finale „{}“: ins Finale kommen nur die Küren von Vorlaufklassen, deren Vorlauf entfällt.",
                            act.name,
                            final_name.unwrap_or_default()
                        )));
                    }
                    (None, Some(direct)) => {
                        let direct = db_sqlite::structure::final_name_of(&mut tx, *direct)
                            .await?
                            .unwrap_or_default();
                        return Err(invalid(&format!(
                            "„{}“ startet direkt im Finale „{direct}“, ihr Vorlauf entfällt.",
                            act.name
                        )));
                    }
                    (None, None) => {}
                }
                if !acts.insert(*act_id) {
                    return Err(invalid("Eine Kür kann nur einmal im Block stehen."));
                }
            }
            BlockSlotInput::Finalist { heat_id, number } => {
                if final_id.is_none() {
                    return Err(invalid("Finalplätze stehen nur im Block ihres Finales."));
                }
                let heat = block.slots[started..].iter().find_map(|slot| match slot {
                    Slot::Finalist(f) if f.heat_id == *heat_id && f.number == *number => {
                        Some(f.group_label.as_str())
                    }
                    _ => None,
                });
                let open = heat.is_some();
                // For the message: the heat's code, also if the slot isn't
                // open.
                let heat_name = match heat {
                    Some(code) => code.to_string(),
                    None => db_sqlite::structure::heat_code_of(&mut tx, *heat_id)
                        .await?
                        .unwrap_or_default(),
                };
                let label = crate::finals::finalist_label(*number, &heat_name);
                if !open {
                    return Err(invalid(&format!(
                        "„{label}“ gibt es in diesem Finale nicht, er hat schon begonnen oder der Vorlauf seiner Vorlaufklasse entfällt."
                    )));
                }
                if !finalists.insert((heat_id, *number)) {
                    return Err(invalid(&format!(
                        "„{label}“ kann nur einmal im Block stehen."
                    )));
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
        sqlx::query!("DELETE FROM timeplan_slots WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;
    }
    // Acts without a category at the moment (withdrawn, incomplete groups)
    // aren't shown, so they aren't sent either: they keep their place.
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
    for (position, input) in body.slots.iter().enumerate() {
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

    // The next act may be another one now.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetBlockOrderResponse {}))
}
