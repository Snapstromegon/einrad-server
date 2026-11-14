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
pub struct LeavePairResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LeavePairBody {
    rider_id: RiderId,
}

/// Takes a rider out of the pair competition: their pair request goes, or
/// their pair act is dissolved and the partner is left waiting for a new
/// partner (a pair request without a name).
///
/// Only the rider's club (or an admin) can do it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/leave_pair",
    request_body=LeavePairBody,
    responses(
        (status=200, content_type="application/json", body=LeavePairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn leave_pair(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<LeavePairBody>,
) -> Result<Json<LeavePairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.leave_pair(&auth.actor(), body.rider_id).await?;
    Ok(Json(LeavePairResponse {}))
}
