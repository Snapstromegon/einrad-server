use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::preview_competition_import::{CompetitionFileBody, parse};
use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    image_fetch::ImageFetcher,
    photos::Photos,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    seed::{self, Applied, Dropped},
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
    /// What the replace dropped. The dropped judge registrations are on the
    /// unmapped list of the judges view, to resolve there.
    pub dropped: Dropped,
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
#[instrument(skip(db, photos, body))]
#[axum::debug_handler]
pub async fn import_competition(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(photos): Extension<Arc<Photos>>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<CompetitionFileBody>,
) -> Result<Json<ImportCompetitionResponse>, HttpError> {
    let file = parse(&body)?;
    let reloadable = db.clone();
    let db = db.get().await.clone();
    let report = seed::apply(&db, Some(&file), true)
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
        ImageFetcher::shared().refresh_in_background(reloadable, None);
    }
    // The poller would notice within seconds; the info board shouldn't wait.
    let competition = Competition::current(&db).await?;
    photos.configure(competition.and_then(|c| c.photos).as_ref());

    Ok(Json(ImportCompetitionResponse { outcome, dropped }))
}
