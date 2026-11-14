use crate::ids::CategoryId;
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};
use db_core::{
    DbHandle,
    categories::{MoveDirection, MoveOutcome},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct MoveCategoryUpResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveCategoryUpBody {
    category_id: CategoryId,
}

/// Move a category up by swapping order with the previous category.
///
/// This endpoint is used to reorder categories.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/move_category_up",
    request_body=MoveCategoryUpBody,
    responses(
        (status=200, content_type="application/json", body=MoveCategoryUpResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn move_category_up(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveCategoryUpBody>,
) -> Result<Json<MoveCategoryUpResponse>, HttpError> {
    match repo
        .move_category(body.category_id, MoveDirection::Up)
        .await?
    {
        MoveOutcome::Moved => Ok(Json(MoveCategoryUpResponse {})),
        MoveOutcome::NotFound => Err(HttpError::StatusCode(StatusCode::NOT_FOUND)),
        // The first (last) category can't move further up.
        MoveOutcome::AtEnd => Err(HttpError::StatusCode(StatusCode::BAD_REQUEST)),
    }
}
