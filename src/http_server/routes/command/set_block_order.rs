use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
    timeplan::BlockSlotInput,
};

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
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetBlockOrderBody>,
) -> Result<Json<SetBlockOrderResponse>, HttpError> {
    repo.set_block_order(body.timeplan_id, &body.slots).await?;

    // The next act may be another one now.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetBlockOrderResponse {}))
}
