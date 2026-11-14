use crate::ids::HeatId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteHeatResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteHeatBody {
    id: HeatId,
}

/// Delete a heat; its categories belong to no heat afterwards.
///
/// Not possible while a final's slots are filled from it.
#[utoipa::path(
    post,
    tags=["command", "heat"],
    path="/delete_heat",
    request_body=DeleteHeatBody,
    responses(
        (status=200, content_type="application/json", body=DeleteHeatResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_heat(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteHeatBody>,
) -> Result<Json<DeleteHeatResponse>, HttpError> {
    repo.delete_heat(body.id).await?;
    Ok(Json(DeleteHeatResponse {}))
}
