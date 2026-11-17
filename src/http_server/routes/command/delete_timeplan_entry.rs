use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteTimeplanEntryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteTimeplanEntryBody {
    id: i64, // Primary key of the timeplan entry to delete
}

/// Delete a timeplan entry.
///
/// This endpoint is used to delete an existing timeplan entry. The acts of a
/// start block go back to their home blocks; it is refused if one of those
/// has ended already.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/delete_timeplan_entry",
    request_body=DeleteTimeplanEntryBody,
    responses(
        (status=200, content_type="application/json", body=DeleteTimeplanEntryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_timeplan_entry(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteTimeplanEntryBody>,
) -> Result<Json<DeleteTimeplanEntryResponse>, HttpError> {
    repo.delete_timeplan_entry(body.id).await?;

    Ok(Json(DeleteTimeplanEntryResponse {}))
}
