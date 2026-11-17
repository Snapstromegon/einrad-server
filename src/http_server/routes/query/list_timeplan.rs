use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError,
        extractor::{auth::Granted, startlist::StartlistAccess},
    },
    roles::permit,
    timeplan::{Audience, TimeplanListEntry},
};

/// List all timeplan entries, in order. Before an admin published the
/// timeplan only the organizer team gets them.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_timeplan",
    responses(
        (status=200, content_type="application/json", body=Vec<TimeplanListEntry>),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_timeplan(
    _: StartlistAccess,
    Extension(repo): Extension<DbHandle>,
    internal: Option<Granted<permit::ViewInternalEntries>>,
) -> Result<Json<Vec<TimeplanListEntry>>, HttpError> {
    // The public gets internal entries as a "Pause", like in the prediction.
    let audience = if internal.is_none() {
        Audience::Public
    } else {
        Audience::OrganizerTeam
    };
    Ok(Json(repo.list_timeplan(audience).await?))
}
