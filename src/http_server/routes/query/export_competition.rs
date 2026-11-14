use axum::{
    Extension,
    http::header,
    response::{IntoResponse, Response},
};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    export,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// The competition as a competition file to download, without club data
/// (see `crate::export`); the import and `--competition` take it unchanged.
/// It contains the photo password, and the logo if it's an SVG. Admin.
#[utoipa::path(
    get,
    tags=["query", "competition"],
    path="/export_competition",
    responses(
        (status=200, content_type="application/toml", body=String),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn export_competition(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Response, HttpError> {
    let export = export::export(repo.as_ref())
        .await?
        .ok_or(HttpError::NotFound)?;
    let content = export.content().map_err(HttpError::ErrorMessages)?;
    let name: String = export
        .file
        .competition
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/toml; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}.toml\""),
            ),
        ],
        content,
    )
        .into_response())
}
