use crate::ids::CategoryId;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    heats::Discipline,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    seed,
};

use super::add_category::check_ages;

#[derive(Debug, Serialize, ToSchema)]
pub struct EditCategoryResponse {
    /// Gaps and overlaps between the categories' birthday bounds, derived
    /// from the determining day, as the import reports them.
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditCategoryBody {
    /// The category to edit.
    category_id: CategoryId,
    new_name: String,
    description: Option<String>,
    /// See `AddCategoryBody::min_age`.
    min_age: Option<f64>,
    /// See `AddCategoryBody::under_age`.
    under_age: Option<f64>,
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
    check_ages(&mut tx, &body.new_name, body.min_age, body.under_age).await?;
    check_heat(&mut tx, &body).await?;

    // A rename is a plain update: everything refers to the category by its ID.
    let updated = sqlx::query!(
        r#"
        UPDATE categories 
        SET 
            name = $1,
            description = $2,
            min_age = $3,
            under_age = $4,
            act_type = $5,
            is_single_male = $6,
            einfahrzeit_seconds = $7,
            act_duration_seconds = $8,
            judge_duration_seconds = $9
        WHERE id = $10
        "#,
        body.new_name,
        body.description,
        body.min_age,
        body.under_age,
        body.act_type,
        body.is_single_male,
        body.einfahrzeit_seconds,
        body.act_duration_seconds,
        body.judge_duration_seconds,
        body.category_id,
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(HttpError::NotFound);
    }

    let warnings = seed::stored_coverage_warnings(&mut tx)
        .await
        .map_err(HttpError::ErrorMessages)?;
    tx.commit().await?;

    Ok(Json(EditCategoryResponse { warnings }))
}

/// A category in a heat keeps its act type and gender (HEAT-1): the heat's
/// other categories and its finals share them. The message is German.
async fn check_heat(
    conn: &mut sqlx::SqliteConnection,
    body: &EditCategoryBody,
) -> Result<(), HttpError> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT heats.name as "heat!", categories.name as "name!", act_type as "act_type: ActType", is_single_male
        FROM categories JOIN heats ON heats.id = categories.heat_id
        WHERE categories.id = $1
        "#,
        body.category_id
    )
    .fetch_optional(conn)
    .await?
    else {
        return Ok(());
    };
    if Discipline::new(row.act_type, row.is_single_male)
        != Discipline::new(body.act_type, body.is_single_male)
    {
        return Err(HttpError::ErrorMessages(format!(
            "{} gehört zur Vorlaufklasse „{}“: Typ und Geschlecht bleiben, solange die Kategorie dort ist.",
            row.name, row.heat
        )));
    }
    Ok(())
}
