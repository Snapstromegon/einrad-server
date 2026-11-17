use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    competition::{Competition, Features},
    http_server::HttpError,
    system_status::Capabilities,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct SystemStatus {
    #[serde(flatten)]
    capabilities: Capabilities,
    /// The current competition; `None` until one is configured.
    competition: Option<Competition>,
    /// Whether the jury registration is on (the competition's feature, else
    /// the build default); everything about judges is hidden without it.
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
#[instrument(skip(repo))]
pub async fn get_system_status(
    Extension(repo): Extension<DbHandle>,
    capabilities: Capabilities,
) -> Result<Json<SystemStatus>, HttpError> {
    let competition = repo.current_competition().await?;
    let features = Features::of(competition.as_ref());
    Ok(Json(SystemStatus {
        capabilities,
        competition,
        jury_registration: features.jury_registration,
        moderation_input: features.moderation_input,
        startlist_published: repo.startlist_published().await?,
    }))
}
