use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
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
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn timeplan_forward(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ControlMusic>,
) -> Result<Json<SetTimeplanForwardResponse>, HttpError> {
    info!("Forwarding timeplan");
    let ended_act = repo.timeplan_forward().await?;
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
