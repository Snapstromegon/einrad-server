use std::sync::Arc;

use axum::{Extension, Json};
use tracing::instrument;

use crate::player::{Playback, PlayerHub};

/// What plays now: the song on the active player, or in the music control's
/// own browser, with its act and position. Public, like the moderation
/// screen that shows it.
#[utoipa::path(
    get,
    tags=["query", "player"],
    path="/get_playback",
    responses(
        (status=200, content_type="application/json", body=Playback),
    ),
)]
#[instrument(skip(hub))]
pub async fn get_playback(Extension(hub): Extension<Arc<PlayerHub>>) -> Json<Playback> {
    Json(hub.playback())
}
