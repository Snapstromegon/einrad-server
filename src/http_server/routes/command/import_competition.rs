use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::preview_competition_import::{CompetitionFileBody, parse};
use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    image_fetch::ImageFetcher,
    photos::Photos,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    seed::{self, Applied, Dropped, RemapFile},
};

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportOutcome {
    /// There was no competition; the file created it (setup mode).
    Created,
    /// The file replaced the competition.
    Replaced,
    /// The file changes nothing.
    Unchanged,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ImportCompetitionResponse {
    pub outcome: ImportOutcome,
    /// What the replace dropped.
    pub dropped: Dropped,
    /// The remap file of the dropped judge registrations, to download, fill
    /// in and apply on the judges screen; `None` without any.
    pub remap_file: Option<RemapFileDownload>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RemapFileDownload {
    pub name: String,
    pub content: String,
}

/// Imports a competition file: creates the competition in setup mode, or
/// replaces it as `--replace-competition` does, keeping its id and the club
/// data (ADR 0005). An invalid file changes nothing. The new settings apply
/// right away, without a restart. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/import_competition",
    request_body=CompetitionFileBody,
    responses(
        (status=200, content_type="application/json", body=ImportCompetitionResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, repo, http_options, photos, body))]
#[axum::debug_handler]
pub async fn import_competition(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(repo): Extension<DbHandle>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Extension(photos): Extension<Arc<Photos>>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<CompetitionFileBody>,
) -> Result<Json<ImportCompetitionResponse>, HttpError> {
    let file = parse(&body)?;
    let db = db.get().await.clone();
    let report = seed::apply(&db, Some(&file), true, &http_options.data_path)
        .await
        .map_err(HttpError::ErrorMessages)?;
    let (outcome, dropped) = match report.applied {
        Applied::Created => (ImportOutcome::Created, Dropped::default()),
        Applied::Replaced(dropped) => (ImportOutcome::Replaced, dropped),
        Applied::NoFile | Applied::Unchanged | Applied::Ignored => {
            (ImportOutcome::Unchanged, Dropped::default())
        }
    };
    info!("Imported the competition file: {outcome:?}");
    // New URL images are downloaded in the background: the import doesn't
    // wait for other servers (#291).
    if !matches!(outcome, ImportOutcome::Unchanged) {
        ImageFetcher::shared().refresh_in_background(repo.clone(), None);
    }
    // The poller would notice within seconds; the info board shouldn't wait.
    let competition = repo.current_competition().await?;
    photos.configure(competition.and_then(|c| c.photos).as_ref());

    let remap_file = if dropped.judge_registrations.is_empty() {
        None
    } else {
        let content = seed::remap_content(&RemapFile {
            jury_registrations: dropped.judge_registrations.clone(),
        })
        .map_err(HttpError::ErrorMessages)?;
        let name = dropped
            .remap_file
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || "jury-registrations.toml".to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
        Some(RemapFileDownload { name, content })
    };
    Ok(Json(ImportCompetitionResponse {
        outcome,
        dropped,
        remap_file,
    }))
}
