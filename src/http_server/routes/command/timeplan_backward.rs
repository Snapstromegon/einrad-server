use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
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
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn timeplan_backward(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ControlMusic>,
) -> Result<Json<SetTimeplanBackwardResponse>, HttpError> {
    repo.timeplan_backward().await?;
    // A remote player gets the song of the act now due; it doesn't start.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }

    Ok(Json(SetTimeplanBackwardResponse {}))
}
