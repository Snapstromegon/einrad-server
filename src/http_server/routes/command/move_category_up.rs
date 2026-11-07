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
pub struct MoveCategoryUpResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveCategoryUpBody {
    name: String,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn move_category_up(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveCategoryUpBody>,
) -> Result<Json<MoveCategoryUpResponse>, HttpError> {
    let db = db.get().await.clone();

    // Start a transaction
    let mut tx = begin_write(&db).await?;

    // Get current category's order
    let current = sqlx::query!(
        r#"
        SELECT "order" FROM categories 
        WHERE name = $1
        "#,
        body.name
    )
    .fetch_optional(&mut *tx)
    .await?;

    let current_record = current.ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;

    // If category has no order, assign it to the end
    let current_order = if let Some(order) = current_record.order {
        order
    } else {
        // Find the maximum order and assign current_order to max + 1
        let max_order = sqlx::query!(
            r#"
            SELECT MAX("order") as max_order FROM categories
            "#
        )
        .fetch_one(&mut *tx)
        .await?
        .max_order
        .unwrap_or(0);

        let new_order = max_order + 1;
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            new_order,
            body.name
        )
        .execute(&mut *tx)
        .await?;

        new_order
    };

    // Find the previous category (the one with the largest order less than current)
    let prev_entry = sqlx::query!(
        r#"
        SELECT name, "order" FROM categories 
        WHERE "order" < $1 
        ORDER BY "order" DESC 
        LIMIT 1
        "#,
        current_order
    )
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(prev) = prev_entry {
        let prev_name = prev.name;
        let prev_order = prev
            .order
            .ok_or(HttpError::StatusCode(StatusCode::BAD_REQUEST))?;

        // Use a temporary negative order to avoid constraint violations
        let temp_order = -999999;

        // Move current to temp
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            temp_order,
            body.name
        )
        .execute(&mut *tx)
        .await?;

        // Move previous to current position
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            current_order,
            prev_name
        )
        .execute(&mut *tx)
        .await?;

        // Move temp to previous position
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            prev_order,
            body.name
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
    } else {
        // No previous entry, can't move up
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }

    Ok(Json(MoveCategoryUpResponse {}))
}
