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
pub struct MoveCategoryDownResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveCategoryDownBody {
    category_id: CategoryId,
}

/// Move a category down by swapping order with the next category.
///
/// This endpoint is used to reorder categories.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/move_category_down",
    request_body=MoveCategoryDownBody,
    responses(
        (status=200, content_type="application/json", body=MoveCategoryDownResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn move_category_down(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveCategoryDownBody>,
) -> Result<Json<MoveCategoryDownResponse>, HttpError> {
    match repo
        .move_category(body.category_id, MoveDirection::Down)
        .await?
    {
        MoveOutcome::Moved => Ok(Json(MoveCategoryDownResponse {})),
        MoveOutcome::NotFound => Err(HttpError::StatusCode(StatusCode::NOT_FOUND)),
        // The first (last) category can't move further down.
        MoveOutcome::AtEnd => Err(HttpError::StatusCode(StatusCode::BAD_REQUEST)),
    }
}
