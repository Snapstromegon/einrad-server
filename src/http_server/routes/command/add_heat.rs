use crate::ids::{CategoryId, HeatId};
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
    /// The readable name, unique, e.g. "Einzel männlich – Expert 1 (U17–U21)".
    name: String,
    /// The short code, unique, for tight spaces and references in files,
    /// e.g. "E EM 1". Without: the name.
    #[serde(default)]
    code: Option<String>,
    description: Option<String>,
    /// All of one act type and gender, none in another heat, group
    /// categories included. May be empty. A heat of groups starts with its
    /// Vorlauf skipped.
    category_ids: Vec<CategoryId>,
    /// The fewest acts it needs for its Vorlauf. Without: 6 if all its
    /// categories are U15 or younger (Junior Expert), otherwise 5.
    #[serde(default)]
    threshold: Option<i64>,
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
        (status=409, content_type="application/json", body=ClientError),
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
    let code = heats::normalize_code(body.code.as_deref(), &name);
    let description = normalize_description(body.description);
    let threshold = body
        .threshold
        .map(heats::check_threshold)
        .transpose()
        .map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let categories = heats::category_names(&mut tx, &body.category_ids)
        .await
        .map_err(HttpError::ErrorMessages)?;

    heats::check_free(&mut tx, None, &name, &code)
        .await
        .map_err(HttpError::Conflict)?;
    let threshold = match threshold {
        Some(threshold) => threshold,
        None => heats::default_threshold_of(&mut tx, &categories).await?,
    };

    info!("Adding heat {name:?}");
    let id = HeatId::new();
    sqlx::query!(
        "INSERT INTO heats (id, name, code, description, threshold) VALUES (?, ?, ?, ?, ?)",
        id,
        name,
        code,
        description,
        threshold
    )
    .execute(&mut *tx)
    .await?;
    // A refusal rolls the new heat back with the transaction.
    heats::check_categories(&mut tx, id, &categories)
        .await
        .map_err(HttpError::ErrorMessages)?;
    heats::set_categories(&mut tx, id, &categories).await?;
    // Groups start in their final: their Vorlauf is skipped from the start
    // (#345); the organizer may restore it.
    if heats::discipline(&mut tx, id)
        .await?
        .is_some_and(heats::Discipline::is_group)
    {
        sqlx::query!("UPDATE heats SET skip = TRUE WHERE id = ?", id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(Json(AddHeatResponse {}))
}

/// Trims a description; empty becomes none.
pub fn normalize_description(description: Option<String>) -> Option<String> {
    description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
}
