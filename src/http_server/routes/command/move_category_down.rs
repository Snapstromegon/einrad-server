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
pub struct MoveCategoryDownResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveCategoryDownBody {
    name: String,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn move_category_down(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveCategoryDownBody>,
) -> Result<Json<MoveCategoryDownResponse>, HttpError> {
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

    // Find the next category (the one with the smallest order greater than current)
    let next_entry = sqlx::query!(
        r#"
        SELECT name, "order" FROM categories 
        WHERE "order" > $1 
        ORDER BY "order" ASC 
        LIMIT 1
        "#,
        current_order
    )
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(next) = next_entry {
        let next_name = next.name;
        let next_order = next
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

        // Move next to current position
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            current_order,
            next_name
        )
        .execute(&mut *tx)
        .await?;

        // Move temp to next position
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE name = $2"#,
            next_order,
            body.name
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
    } else {
        // No next entry, can't move down
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }

    Ok(Json(MoveCategoryDownResponse {}))
}
