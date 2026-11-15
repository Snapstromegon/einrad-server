use std::sync::Arc;

use crate::ids::{ActId, HeatId};

use axum::{Extension, Json};
use db_core::{
    commands::timeplan::{self as cmd, SetBlockOrder},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
};

/// One slot of a start block, as sent to `set_block_order`.
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

impl From<BlockSlotInput> for cmd::BlockSlotInput {
    fn from(slot: BlockSlotInput) -> Self {
        match slot {
            BlockSlotInput::Act { act_id } => Self::Act { act_id },
            BlockSlotInput::Finalist { heat_id, number } => Self::Finalist { heat_id, number },
            BlockSlotInput::Break {
                id,
                label,
                duration_seconds,
            } => Self::Break {
                id,
                label,
                duration_seconds,
            },
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SetBlockOrderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBlockOrderBody {
    timeplan_id: i64,
    /// The complete order of the block. Its started and ended slots must
    /// come first, as they are.
    slots: Vec<BlockSlotInput>,
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
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn set_block_order(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    auth: Granted<permit::PlanCompetition>,
    Json(body): Json<SetBlockOrderBody>,
) -> Result<Json<SetBlockOrderResponse>, HttpError> {
    repo.set_block_order(
        &auth.actor(),
        SetBlockOrder {
            timeplan_id: body.timeplan_id,
            slots: body.slots.into_iter().map(Into::into).collect(),
        },
    )
    .await?;

    // The next act may be another one now.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetBlockOrderResponse {}))
}
