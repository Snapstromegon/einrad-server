use crate::ids::ActId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetActMusicStartedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ActMusicStartedBody {
    act_id: ActId,
}

/// Reports that the music of a running act just started in the music
/// control's own browser; remote players report it themselves. Only the first
/// start counts, and acts that don't run are left alone.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/set_act_music_started",
    request_body=ActMusicStartedBody,
    responses(
        (status=200, content_type="application/json", body=SetActMusicStartedResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn set_act_music_started(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ControlMusic>,
    Json(body): Json<ActMusicStartedBody>,
) -> Result<Json<SetActMusicStartedResponse>, HttpError> {
    if repo.mark_music_started(body.act_id, 0.0).await? {
        info!("Music of act {} started", body.act_id);
    }
    Ok(Json(SetActMusicStartedResponse {}))
}
