use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    seed,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddCategoryResponse {
    /// Gaps and overlaps between the categories' birthday bounds, derived
    /// from the determining day, as the import reports them.
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddCategoryBody {
    name: String,
    description: Option<String>,
    /// The age on the determining day from which on riders belong to the
    /// category, e.g. 70 for "70+"; `None` for no lower age. Fractions are
    /// months: 9.5 is 9 years 6 months.
    min_age: Option<f64>,
    /// The age riders are under on the determining day, e.g. 11 for "U11";
    /// `None` for no upper age.
    under_age: Option<f64>,
    act_type: ActType,
    is_single_male: bool,
    einfahrzeit_seconds: i32,
    act_duration_seconds: i32,
    judge_duration_seconds: i32,
}

/// Add a new category, defined by its age class: the birthday bounds follow
/// from the determining day.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/add_category",
    request_body=AddCategoryBody,
    responses(
        (status=200, content_type="application/json", body=AddCategoryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_category(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddCategoryBody>,
) -> Result<Json<AddCategoryResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    check_ages(&mut transaction, &body.name, body.min_age, body.under_age).await?;

    sqlx::query!(
        r#"
        INSERT INTO categories (
            name, 
            description, 
            min_age,
            under_age,
            act_type, 
            is_single_male, 
            einfahrzeit_seconds, 
            act_duration_seconds, 
            judge_duration_seconds,
            "order"
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9,
            (SELECT COALESCE(MAX("order"), 0) + 1 FROM categories)
        )
        "#,
        body.name,
        body.description,
        body.min_age,
        body.under_age,
        body.act_type,
        body.is_single_male,
        body.einfahrzeit_seconds,
        body.act_duration_seconds,
        body.judge_duration_seconds,
    )
    .execute(&mut *transaction)
    .await?;
    let warnings = seed::stored_coverage_warnings(&mut transaction)
        .await
        .map_err(HttpError::ErrorMessages)?;
    transaction.commit().await?;

    Ok(Json(AddCategoryResponse { warnings }))
}

/// Rejects ages the import rejects, e.g. a `min_age` not below `under_age`.
pub(super) async fn check_ages(
    conn: &mut sqlx::SqliteConnection,
    name: &str,
    min_age: Option<f64>,
    under_age: Option<f64>,
) -> Result<(), HttpError> {
    let determining_day = Competition::current(conn)
        .await?
        .ok_or(HttpError::NotFound)?
        .determining_day;
    seed::birthday_bounds(name, min_age, under_age, determining_day)
        .map_err(HttpError::ErrorMessages)?;
    Ok(())
}
