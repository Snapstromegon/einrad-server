use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ClearActSongResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ClearActSongBody {
    act_id: Uuid,
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
    Extension(db): Extension<ReloadableSqlite>,
    auth: Granted<permit::ManageUsers>,
    Json(body): Json<ClearActSongBody>,
) -> Result<Json<ClearActSongResponse>, HttpError> {
    let db = db.get().await.clone();
    let done = sqlx::query(
        r#"
        UPDATE acts
        SET song_file = NULL, song_file_name = NULL, song_file_normalized = NULL,
            normalization_status = NULL, song_checked = FALSE
        WHERE id = ?
        "#,
    )
    .bind(body.act_id)
    .execute(&db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(HttpError::NotFound);
    }
    info!(
        "{} marked the song of act {} as permanently deleted",
        auth.name, body.act_id
    );
    Ok(Json(ClearActSongResponse {}))
}
