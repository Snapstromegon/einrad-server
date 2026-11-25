//! `GET /api/query/get_song?file=<name>`: the music files of the data folder,
//! for logged-in users.
//!
//! The data folder holds more than songs, so only file names that look like
//! a song are served: `<act-uuid>.mp3` (an upload), `<act-uuid>.wav` (the
//! demo's tone) and `<act-uuid>.<uuid>.normalized.mp3` (the loudness-
//! normalized copy). Everything else is 404.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Extension,
    body::Body,
    extract::{Query, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tracing::instrument;
use uuid::Uuid;

use crate::http_server::{ClientError, HttpServerOptions, extractor::auth::Auth};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetSongQuery {
    /// The song's file name, e.g. `<act-id>.mp3` (an act's `song_file` or
    /// `song_file_normalized`).
    file: String,
}

/// The song's bytes, for the OpenAPI spec.
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct SongBytes(Vec<u8>);

/// A hyphenated UUID, the form the file names use.
fn is_uuid(part: &str) -> bool {
    part.len() == 36 && Uuid::parse_str(part).is_ok()
}

/// Whether `name` is a song file name (see the module docs).
fn is_song_file_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    match parts.as_slice() {
        [act, "mp3" | "wav"] => is_uuid(act),
        [act, version, "normalized", "mp3"] => is_uuid(act) && is_uuid(version),
        _ => false,
    }
}

/// Download a song file. Logged in users.
///
/// Supports range requests (seeking) and conditional requests.
#[utoipa::path(
    get,
    tags=["query", "act"],
    path="/get_song",
    params(GetSongQuery),
    responses(
        (status=200, content((SongBytes = "audio/mpeg"), (SongBytes = "audio/wav"))),
        (status=206, content((SongBytes = "audio/mpeg"), (SongBytes = "audio/wav"))),
        (status=401, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(options, request))]
pub async fn get_song(
    _: Auth,
    Extension(options): Extension<Arc<HttpServerOptions>>,
    Query(query): Query<GetSongQuery>,
    request: Request,
) -> Response {
    if !is_song_file_name(&query.file) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path: PathBuf = options.data_path.join(&query.file);
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::is_song_file_name;

    const ACT: &str = "01a20a43-bdf4-7319-8cbc-59970d8653c6";
    const VERSION: &str = "01a20a43-bdf5-7000-8000-000000000001";

    #[test]
    fn only_song_names_pass() {
        for ok in [
            format!("{ACT}.mp3"),
            format!("{ACT}.wav"),
            format!("{ACT}.{VERSION}.normalized.mp3"),
        ] {
            assert!(is_song_file_name(&ok), "{ok}");
        }
        for bad in [
            "x.toml".to_string(),
            "remap".to_string(),
            "..".to_string(),
            format!("{ACT}.toml"),
            format!("{ACT}.mp3.bak"),
            format!("{ACT}.{VERSION}.normalized.wav"),
            format!("{ACT}.{VERSION}.mp3"),
            format!(".{ACT}.mp3"),
            format!("{}.mp3", ACT.replace('-', "")),
            format!("{{{ACT}}}.mp3"),
            String::new(),
        ] {
            assert!(!is_song_file_name(&bad), "{bad}");
        }
    }
}
