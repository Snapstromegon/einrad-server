use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Category {
    name: String,
    description: Option<String>,
    /// The age on the determining day from which on riders belong to the
    /// category; `None` for no lower age. Fractions are months.
    min_age: Option<f64>,
    /// The age riders are under on the determining day; `None` for no
    /// upper age.
    under_age: Option<f64>,
    act_type: ActType,
    is_single_male: bool,
    order: Option<i64>,
    einfahrzeit_seconds: Option<i64>,
    act_duration_seconds: Option<i64>,
    judge_duration_seconds: Option<i64>,
}

/// Get information about a club.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_categories",
    responses(
        (status=200, content_type="application/json", body=Vec<Category>),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_categories(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Option<Auth>,
) -> Result<Json<Vec<Category>>, HttpError> {
    let db = db.get().await.clone();
    let club_categories = sqlx::query_as!(
        Category,
        r#"
        SELECT name as "name!", description, min_age, under_age, act_type as "act_type: ActType", is_single_male, "order", einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
        FROM categories ORDER BY "order" ASC
        "#
    )
    .fetch_all(&db)
    .await?;
    Ok(Json(club_categories))
}
