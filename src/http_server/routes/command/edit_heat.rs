use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::add_heat::normalize_description;
use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditHeatResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditHeatBody {
    /// The heat to edit.
    name: String,
    new_name: String,
    description: Option<String>,
    /// See `AddHeatBody::categories`; the ones left out leave the heat.
    categories: Vec<String>,
    /// The fewest acts it needs for its Vorlauf; without, it stays.
    #[serde(default)]
    threshold: Option<i64>,
}

/// Edit a heat: its name, description, categories and threshold.
///
/// A rename carries over to the finals' slots. The categories keep the
/// discipline of the finals the heat fills. Acts already placed in a final
/// stay when their category leaves the heat.
#[utoipa::path(
    post,
    tags=["command", "heat"],
    path="/edit_heat",
    request_body=EditHeatBody,
    responses(
        (status=200, content_type="application/json", body=EditHeatResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_heat(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditHeatBody>,
) -> Result<Json<EditHeatResponse>, HttpError> {
    let new_name = heats::normalize_name(&body.new_name).map_err(HttpError::ErrorMessages)?;
    let description = normalize_description(body.description);
    let categories = heats::normalize_categories(&body.categories);
    let threshold = body
        .threshold
        .map(heats::check_threshold)
        .transpose()
        .map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM heats WHERE name = ?) as "exists!: bool""#,
        body.name
    )
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    heats::check_categories(&mut tx, &body.name, &categories)
        .await
        .map_err(HttpError::ErrorMessages)?;
    heats::set_categories(&mut tx, &body.name, &categories).await?;

    if new_name != body.name {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM heats WHERE name = ?) as "taken!: bool""#,
            new_name
        )
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(HttpError::ErrorMessages(format!(
                "Eine Vorlaufklasse „{new_name}“ gibt es schon."
            )));
        }
        info!("Renaming heat {:?} to {new_name:?}", body.name);
    }
    // The categories and slots follow a rename by their foreign keys.
    sqlx::query!(
        "UPDATE heats SET name = ?, description = ?, threshold = coalesce(?, threshold) WHERE name = ?",
        new_name,
        description,
        threshold,
        body.name
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(EditHeatResponse {}))
}
