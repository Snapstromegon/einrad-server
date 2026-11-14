use crate::ids::FinalId;
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
pub struct SetFinalPublishedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFinalPublishedBody {
    /// The final.
    id: FinalId,
    published: bool,
}

/// Publish (or withdraw) the placement of a final.
///
/// Once published, the timeplan shows the placed acts in the final's slots
/// and the player plays their songs. It can't be withdrawn once the final
/// has started.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/set_final_published",
    request_body=SetFinalPublishedBody,
    responses(
        (status=200, content_type="application/json", body=SetFinalPublishedResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn set_final_published(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ManageResults>,
    Json(body): Json<SetFinalPublishedBody>,
) -> Result<Json<SetFinalPublishedResponse>, HttpError> {
    repo.set_final_published(body.id, body.published).await?;
    // The final's songs may be coming up now (or no longer).
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetFinalPublishedResponse {}))
}
