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
pub struct SetSongCheckedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SongCheckedBody {
    act_id: ActId,
    checked: bool,
}

/// EditClubAct a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/set_song_checked",
    request_body=SongCheckedBody,
    responses(
        (status=200, content_type="application/json", body=SetSongCheckedResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_song_checked(
    Extension(db): Extension<DbHandle>,
    _: Granted<permit::CheckSongs>,
    Json(body): Json<SongCheckedBody>,
) -> Result<Json<SetSongCheckedResponse>, HttpError> {
    info!(
        "Setting song checked to {} for act {}",
        body.checked, body.act_id
    );

    db.set_song_checked(body.act_id, body.checked).await?;

    Ok(Json(SetSongCheckedResponse {}))
}
