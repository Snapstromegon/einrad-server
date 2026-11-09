use std::sync::Arc;

use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    competition::{Competition, Features},
    http_server::{HttpError, HttpServerOptions},
    reloadable_sqlite::ReloadableSqlite,
    system_status::Capabilities,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct SystemStatus {
    #[serde(flatten)]
    capabilities: Capabilities,
    /// The current competition; `None` until one is configured.
    competition: Option<Competition>,
    /// Whether the jury registration is on (the competition's feature, else
    /// the system default); everything about judges is hidden without it.
    jury_registration: bool,
    /// Whether the act description field is enabled for the current competition.
    moderation_input: bool,
    /// Whether an admin published the start list and the timeplan; before,
    /// only the organizer team sees them.
    startlist_published: bool,
}

/// Get information about the status of the system and the current competition.
#[utoipa::path(
    get,
    tags=["query"],
    path="/get_system_status",
    responses(
        (status=200, content_type="application/json", body=SystemStatus),
    ),
)]
#[instrument(skip(db, http_options))]
pub async fn get_system_status(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    capabilities: Capabilities,
) -> Result<Json<SystemStatus>, HttpError> {
    let db = db.get().await.clone();
    let competition = Competition::current(&db).await?;
    let features = Features::of(competition.as_ref(), &http_options.features);
    Ok(Json(SystemStatus {
        capabilities,
        competition,
        jury_registration: features.jury_registration,
        moderation_input: features.moderation_input,
        startlist_published: Competition::startlist_published(&db).await?,
    }))
}
