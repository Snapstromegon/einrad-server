use axum::{Extension, Json};
use db_core::{commands::finals::AddFinal, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    finals, heats,
    http_server::{
        ClientError, HttpError, extractor::auth::Granted, routes::http_types::FinalFields,
    },
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddFinalBody {
    /// The readable name, unique, e.g. "Finale Einzel männlich Expert".
    name: String,
    /// The short code, unique, for tight spaces and references in files,
    /// e.g. "E EM". Without: the name.
    #[serde(default)]
    code: Option<String>,
    #[serde(flatten)]
    fields: FinalFields,
}

/// Add a final: reserved slots for the best riders of the heats.
///
/// It isn't planned yet; add a timeplan entry with its `final_id` for that.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/add_final",
    request_body=AddFinalBody,
    responses(
        (status=200, content_type="application/json", body=AddFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn add_final(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::PlanCompetition>,
    Json(body): Json<AddFinalBody>,
) -> Result<Json<AddFinalResponse>, HttpError> {
    let name = finals::normalize_name(&body.name).map_err(HttpError::ErrorMessages)?;
    let code = heats::normalize_code(body.code.as_deref(), &name);
    let fields = db_core::finals::FinalFields::from(body.fields)
        .normalize()
        .map_err(HttpError::ErrorMessages)?;
    repo.add_final(&granted.actor(), AddFinal { name, code, fields })
        .await?;
    Ok(Json(AddFinalResponse {}))
}
