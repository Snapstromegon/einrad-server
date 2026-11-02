use axum::{
    Extension,
    http::header,
    response::{IntoResponse, Response},
};
use tracing::instrument;

use crate::{
    export,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// The competition as a competition file to download, without club data
/// (see `crate::export`); the import and `--competition` take it unchanged.
/// It contains the photo password. Admin.
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
#[instrument(skip(db))]
pub async fn export_competition(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Response, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    let file = export::export(&mut conn)
        .await?
        .ok_or(HttpError::NotFound)?;
    let content = export::to_file_content(&file).map_err(HttpError::ErrorMessages)?;
    let name: String = file
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
