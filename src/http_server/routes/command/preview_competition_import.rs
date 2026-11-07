use axum::{Extension, Json};
use serde::Deserialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    config::CompetitionFile,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    seed::{self, Preview},
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompetitionFileBody {
    /// The competition file's content (TOML), as `--competition` takes it.
    pub file: String,
}

/// Parses a competition file the way `import_competition` does.
pub(super) fn parse(body: &CompetitionFileBody) -> Result<CompetitionFile, HttpError> {
    CompetitionFile::parse(&body.file)
        .map_err(|e| HttpError::ErrorMessages(format!("Ungültige Wettkampfdatei: {e}")))
}

/// What importing a competition file would replace and drop, without
/// changing anything; invalid files fail as the import would. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/preview_competition_import",
    request_body=CompetitionFileBody,
    responses(
        (status=200, content_type="application/json", body=Preview),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn preview_competition_import(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<CompetitionFileBody>,
) -> Result<Json<Preview>, HttpError> {
    let file = parse(&body)?;
    let db = db.get().await.clone();
    let preview = seed::preview(&db, &file)
        .await
        .map_err(HttpError::ErrorMessages)?;
    Ok(Json(preview))
}
