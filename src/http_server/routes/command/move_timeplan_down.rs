use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct MoveTimeplanDownResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveTimeplanDownBody {
    id: i64,
}

/// Move a timeplan entry down by swapping places with the next entry.
///
/// This endpoint is used to reorder timeplan entries.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/move_timeplan_down",
    request_body=MoveTimeplanDownBody,
    responses(
        (status=200, content_type="application/json", body=MoveTimeplanDownResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn move_timeplan_down(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveTimeplanDownBody>,
) -> Result<Json<MoveTimeplanDownResponse>, HttpError> {
    if !repo.move_timeplan_entry(body.id, false).await? {
        // No neighbour to swap with.
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }

    Ok(Json(MoveTimeplanDownResponse {}))
}
