use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditCategoryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditCategoryBody {
    name: String, // Primary key - category to edit
    new_name: String,
    description: Option<String>,
    from_birthday: String, // ISO date string
    to_birthday: String,   // ISO date string
    act_type: ActType,
    is_single_male: bool,
    einfahrzeit_seconds: i32,
    act_duration_seconds: i32,
    judge_duration_seconds: i32,
}

/// Edit an existing category.
///
/// This endpoint is used to update a category's properties.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/edit_category",
    request_body=EditCategoryBody,
    responses(
        (status=200, content_type="application/json", body=EditCategoryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_category(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditCategoryBody>,
) -> Result<Json<EditCategoryResponse>, HttpError> {
    let db = db.get().await.clone();

    // Start a transaction to ensure atomicity
    let mut tx = begin_write(&db).await?;

    sqlx::query!("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await?;

    // Update timeplan table first to maintain foreign key constraint
    if body.new_name != body.name {
        sqlx::query!(
            r#"
            UPDATE timeplan 
            SET category = $1 
            WHERE category = $2
            "#,
            body.new_name,
            body.name,
        )
        .execute(&mut *tx)
        .await?;
    }

    // Update the category
    sqlx::query!(
        r#"
        UPDATE categories 
        SET 
            name = $1,
            description = $2,
            from_birthday = $3,
            to_birthday = $4,
            act_type = $5,
            is_single_male = $6,
            einfahrzeit_seconds = $7,
            act_duration_seconds = $8,
            judge_duration_seconds = $9
        WHERE name = $10
        "#,
        body.new_name,
        body.description,
        body.from_birthday,
        body.to_birthday,
        body.act_type,
        body.is_single_male,
        body.einfahrzeit_seconds,
        body.act_duration_seconds,
        body.judge_duration_seconds,
        body.name,
    )
    .execute(&mut *tx)
    .await?;

    // Commit the transaction
    tx.commit().await?;

    Ok(Json(EditCategoryResponse {}))
}
