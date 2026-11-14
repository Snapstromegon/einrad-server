use crate::ids::ActId;
use std::{collections::HashMap, path::Path, sync::Arc};

use axum::{
    Extension, Json,
    extract::{Multipart, Query},
    http::StatusCode,
};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tokio::fs::DirBuilder;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    normalize,
    reloadable_sqlite::ReloadableSqlite,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SaveActSongResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveActSongQuery {
    act_id: ActId,
}

/// The largest song file a club can upload.
pub const MAX_SONG_SIZE: usize = 10 * 1024 * 1024;
/// The request limit for commands: a song plus the multipart overhead.
/// Image uploads have none (ADR 0010).
pub const MAX_REQUEST_SIZE: usize = MAX_SONG_SIZE + 1024 * 1024;

pub type Metadata = HashMap<String, String>;

#[derive(ToSchema, Debug)]
pub struct Upload {
    #[allow(dead_code)]
    #[schema(value_type = String, format = Binary)]
    pub file_content: Vec<u8>,
    #[allow(dead_code)]
    pub metadata: Option<Metadata>,
}

/// Upload an act's song.
///
/// Replaces an earlier upload, which also resets `song_checked`: the new file
/// hasn't been checked yet. Only clubs with a rider in the act, the club that
/// created a group, and admins can upload; clubs only until the music
/// deadline. Only mp3 files up to 10 MiB are accepted. A loudness-normalized
/// copy is made in the background (see `normalize`).
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/save_act_song",
    request_body=Upload,
    responses(
        (status=200, content_type="application/json", body=SaveActSongResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, db))]
#[axum::debug_handler]
pub async fn save_act_song(
    Extension(repo): Extension<DbHandle>,
    Extension(db): Extension<ReloadableSqlite>,
    http_options: Extension<Arc<HttpServerOptions>>,
    auth: Auth,
    capabilities: Capabilities,
    Query(query): Query<SaveActSongQuery>,
    mut body: Multipart,
) -> Result<Json<SaveActSongResponse>, HttpError> {
    if !capabilities.can_upload_music {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    repo.ensure_act_access(&auth.actor(), query.act_id).await?;
    let reloadable_db = db.clone();
    let db = db.get().await.clone();
    let entry = body
        .next_field()
        .await
        .map_err(|e| HttpError::StatusCode(e.status()))?
        .ok_or(HttpError::StatusCode(StatusCode::BAD_REQUEST))?;

    let file_name = entry
        .file_name()
        .ok_or(HttpError::StatusCode(StatusCode::BAD_REQUEST))?
        .to_string();

    // The announcement asks for mp3 files. The client shows the message.
    let is_mp3 = Path::new(&file_name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"));
    if !is_mp3 {
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }

    let save_file_name = format!("{}.mp3", query.act_id);

    let path = http_options.data_path.join(&save_file_name);
    // A body over the request limit fails here, as 413.
    let data = entry
        .bytes()
        .await
        .map_err(|e| HttpError::StatusCode(e.status()))?;
    if data.len() > MAX_SONG_SIZE {
        return Err(HttpError::StatusCode(StatusCode::PAYLOAD_TOO_LARGE));
    }
    DirBuilder::new()
        .recursive(true)
        .create(path.parent().unwrap())
        .await
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    // Unlike `File::write_all`, this only returns once the whole song is
    // written: the normalization below reads it right away.
    tokio::fs::write(&path, &data)
        .await
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let previous_normalized = sqlx::query_scalar!(
        "SELECT song_file_normalized FROM acts WHERE id = $1",
        query.act_id
    )
    .fetch_optional(&db)
    .await?
    .flatten();
    let normalized = normalize::normalized_file_name(query.act_id);
    sqlx::query!(
        r#"
        UPDATE acts
        SET song_file_name = $1, song_file = $2, song_checked = FALSE,
            song_file_normalized = $3, normalization_status = 'pending'
        WHERE id = $4
        "#,
        file_name,
        save_file_name,
        normalized,
        query.act_id,
    )
    .execute(&db)
    .await?;
    if let Some(previous) = previous_normalized {
        normalize::remove_file(&http_options.data_path.join(previous)).await;
    }
    normalize::spawn(
        reloadable_db,
        http_options.data_path.clone(),
        query.act_id,
        save_file_name,
        normalized,
    );

    Ok(Json(SaveActSongResponse {}))
}
