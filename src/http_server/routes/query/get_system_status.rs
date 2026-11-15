use axum::{Extension, Json};
use db_core::{competition::Features, prelude::*};
use tracing::instrument;

use super::super::http_types::Competition;
use crate::{http_server::HttpError, system_status::Capabilities};

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
    let status = repo.get_system_status().await?;
    let features = Features::of(status.competition.as_ref());
    Ok(Json(SystemStatus {
        capabilities,
        competition: status.competition.map(Into::into),
        jury_registration: features.jury_registration,
        moderation_input: features.moderation_input,
        startlist_published: status.startlist_published,
    }))
}
