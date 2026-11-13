use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

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
    id: Uuid,
    new_name: String,
    /// The new short code; without, it stays.
    #[serde(default)]
    new_code: Option<String>,
    description: Option<String>,
    /// See `AddHeatBody::categories`; the ones left out leave the heat.
    categories: Vec<String>,
    /// The fewest acts it needs for its Vorlauf; without, it stays.
    #[serde(default)]
    threshold: Option<i64>,
}

/// Edit a heat: its name, description, categories and threshold.
///
/// A rename is only a new name: everything that refers to the heat does so
/// by its id. The categories keep the
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

    let Some(name) = heats::name_of(&mut tx, body.id).await? else {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    };
    // Without a new code the heat keeps its own.
    let new_code = match body.new_code.as_deref() {
        Some(code) if !code.trim().is_empty() => heats::normalize_code(Some(code), &new_name),
        _ => {
            sqlx::query_scalar!(r#"SELECT code as "code!" FROM heats WHERE id = ?"#, body.id)
                .fetch_one(&mut *tx)
                .await?
        }
    };
    heats::check_categories(&mut tx, body.id, &categories)
        .await
        .map_err(HttpError::ErrorMessages)?;
    heats::set_categories(&mut tx, body.id, &categories).await?;

    heats::check_free(&mut tx, Some(body.id), &new_name, &new_code)
        .await
        .map_err(HttpError::ErrorMessages)?;
    if new_name != name {
        info!("Renaming heat {name:?} to {new_name:?}");
    }
    sqlx::query!(
        "UPDATE heats SET name = ?, code = ?, description = ?, threshold = coalesce(?, threshold) WHERE id = ?",
        new_name,
        new_code,
        description,
        threshold,
        body.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(EditHeatResponse {}))
}
