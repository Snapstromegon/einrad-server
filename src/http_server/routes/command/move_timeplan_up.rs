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
pub struct MoveTimeplanUpResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveTimeplanUpBody {
    id: i64,
}

/// Move a timeplan entry up by swapping places with the previous entry.
///
/// This endpoint is used to reorder timeplan entries.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/move_timeplan_up",
    request_body=MoveTimeplanUpBody,
    responses(
        (status=200, content_type="application/json", body=MoveTimeplanUpResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn move_timeplan_up(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveTimeplanUpBody>,
) -> Result<Json<MoveTimeplanUpResponse>, HttpError> {
    if !repo.move_timeplan_entry(body.id, true).await? {
        // No neighbour to swap with.
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }

    Ok(Json(MoveTimeplanUpResponse {}))
}
