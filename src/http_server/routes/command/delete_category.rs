use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteCategoryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteCategoryBody {
    name: String, // Primary key of the category to delete
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_category(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteCategoryBody>,
) -> Result<Json<DeleteCategoryResponse>, HttpError> {
    let db = db.get().await.clone();

    // Start a transaction to ensure atomicity
    let mut tx = begin_write(&db).await?;

    // Delete timeplan entries that reference this category
    sqlx::query!(
        r#"
        DELETE FROM timeplan WHERE category = $1
        "#,
        body.name,
    )
    .execute(&mut *tx)
    .await?;

    // Delete the category
    let result = sqlx::query!(
        r#"
        DELETE FROM categories
        WHERE name = $1
        "#,
        body.name,
    )
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }

    // Commit the transaction
    tx.commit().await?;

    Ok(Json(DeleteCategoryResponse {}))
}
