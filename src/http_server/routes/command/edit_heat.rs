use crate::ids::{CategoryId, HeatId};
use axum::{Extension, Json};
use db_core::{DbHandle, heats::HeatChange};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use super::add_heat::normalize_description;
use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditHeatResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditHeatBody {
    /// The heat to edit.
    id: HeatId,
    new_name: String,
    /// The new short code; without, it stays.
    #[serde(default)]
    new_code: Option<String>,
    description: Option<String>,
    /// See `AddHeatBody::categories`; the ones left out leave the heat.
    category_ids: Vec<CategoryId>,
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
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_heat(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditHeatBody>,
) -> Result<Json<EditHeatResponse>, HttpError> {
    let new_name = heats::normalize_name(&body.new_name).map_err(HttpError::ErrorMessages)?;
    let description = normalize_description(body.description);
    let threshold = body
        .threshold
        .map(heats::check_threshold)
        .transpose()
        .map_err(HttpError::ErrorMessages)?;
    // Without a new code the heat keeps its own.
    let new_code = body
        .new_code
        .as_deref()
        .filter(|code| !code.trim().is_empty())
        .map(|code| heats::normalize_code(Some(code), &new_name));
    repo.edit_heat(&HeatChange {
        id: body.id,
        new_name,
        new_code,
        description,
        category_ids: body.category_ids,
        threshold,
    })
    .await?;
    Ok(Json(EditHeatResponse {}))
}
