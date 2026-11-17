use crate::ids::RiderId;
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteRiderBody {
    rider_id: RiderId,
}

/// Delete a rider.
///
/// Only the rider's club (or an admin) can delete them. Their single and
/// pair acts are deleted too; the partner is left with a pair request. Groups
/// keep their other riders.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/delete_rider",
    request_body=DeleteRiderBody,
    responses(
        (status=200, content_type="application/json", body=DeleteRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_rider(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteRiderBody>,
) -> Result<Json<DeleteRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.delete_rider(&auth.actor(), body.rider_id).await?;

    Ok(Json(DeleteRiderResponse {}))
}
