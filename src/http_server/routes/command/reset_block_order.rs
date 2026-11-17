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
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ResetBlockOrderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetBlockOrderBody {
    timeplan_id: i64,
}

/// Puts a start block back to category order.
///
/// Deletes the block's stored order and its breaks. Acts that came from
/// other blocks go back to their home blocks, unless one of those has ended;
/// acts of its category that sit in other blocks stay there. A final's block
/// gets its slots' order back, followed by the acts of its skipped heats.
/// Blocks that have started keep their order.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/reset_block_order",
    request_body=ResetBlockOrderBody,
    responses(
        (status=200, content_type="application/json", body=ResetBlockOrderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn reset_block_order(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<ResetBlockOrderBody>,
) -> Result<Json<ResetBlockOrderResponse>, HttpError> {
    repo.reset_block_order(body.timeplan_id).await?;

    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(ResetBlockOrderResponse {}))
}
