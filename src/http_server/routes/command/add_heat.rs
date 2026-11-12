use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddHeatResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddHeatBody {
    /// Unique, e.g. "E EM 1".
    name: String,
    description: Option<String>,
    /// All of one act type and gender, none in another heat, no group
    /// categories. May be empty.
    categories: Vec<String>,
}

/// Add a heat: a group of categories of one discipline whose best acts fill
/// a final's slots.
#[utoipa::path(
    post,
    tags=["command", "heat"],
    path="/add_heat",
    request_body=AddHeatBody,
    responses(
        (status=200, content_type="application/json", body=AddHeatResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_heat(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddHeatBody>,
) -> Result<Json<AddHeatResponse>, HttpError> {
    let name = heats::normalize_name(&body.name).map_err(HttpError::ErrorMessages)?;
    let description = normalize_description(body.description);
    let categories = heats::normalize_categories(&body.categories);
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM heats WHERE name = ?) as "taken!: bool""#,
        name
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(HttpError::ErrorMessages(format!(
            "Eine Vorlaufklasse „{name}“ gibt es schon."
        )));
    }
    heats::check_categories(&mut tx, &name, &categories)
        .await
        .map_err(HttpError::ErrorMessages)?;

    info!("Adding heat {name:?}");
    sqlx::query!(
        "INSERT INTO heats (name, description) VALUES (?, ?)",
        name,
        description
    )
    .execute(&mut *tx)
    .await?;
    heats::set_categories(&mut tx, &name, &categories).await?;

    tx.commit().await?;
    Ok(Json(AddHeatResponse {}))
}

/// Trims a description; empty becomes none.
pub fn normalize_description(description: Option<String>) -> Option<String> {
    description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
}
