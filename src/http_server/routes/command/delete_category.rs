use crate::ids::CategoryId;
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};
use db_core::DbHandle;

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteCategoryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteCategoryBody {
    category_id: CategoryId,
}

/// Delete a category.
///
/// This endpoint is used to delete an existing category.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/delete_category",
    request_body=DeleteCategoryBody,
    responses(
        (status=200, content_type="application/json", body=DeleteCategoryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_category(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteCategoryBody>,
) -> Result<Json<DeleteCategoryResponse>, HttpError> {
    if !repo.delete_category(body.category_id).await? {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    Ok(Json(DeleteCategoryResponse {}))
}
