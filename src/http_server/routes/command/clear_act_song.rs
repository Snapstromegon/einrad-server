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
pub struct ClearActSongResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ClearActSongBody {
    act_id: ActId,
}

/// Mark an act's song as permanently deleted.
///
/// Clears the song fields and `song_checked`, so the act counts as having no
/// song and the usual flows ask for a new upload. Files on disk are left
/// alone. Admins only; meant for songs that `list_missing_songs` reports and
/// that can't be uploaded again.
#[utoipa::path(
    post,
    tags=["command", "admin"],
    path="/clear_act_song",
    request_body=ClearActSongBody,
    responses(
        (status=200, content_type="application/json", body=ClearActSongResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn clear_act_song(
    Extension(db): Extension<DbHandle>,
    auth: Granted<permit::ManageBackups>,
    Json(body): Json<ClearActSongBody>,
) -> Result<Json<ClearActSongResponse>, HttpError> {
    if !db.clear_act_song(body.act_id).await? {
        return Err(HttpError::NotFound);
    }
    info!(
        "{} marked the song of act {} as permanently deleted",
        auth.name, body.act_id
    );
    Ok(Json(ClearActSongResponse {}))
}
