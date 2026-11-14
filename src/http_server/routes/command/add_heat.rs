use crate::ids::CategoryId;
use axum::{Extension, Json};
use db_core::{DbHandle, heats::NewHeat};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
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
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn add_heat(
    Extension(repo): Extension<DbHandle>,
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
    repo.add_heat(&NewHeat {
        name,
        code,
        description,
        category_ids: body.category_ids,
        threshold,
    })
    .await?;
    Ok(Json(AddHeatResponse {}))
}
/// Trims a description; empty becomes none.
pub fn normalize_description(description: Option<String>) -> Option<String> {
    description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
}
