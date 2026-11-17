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
pub struct SetSingleResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetSingleBody {
    rider_id: RiderId,
    /// Whether the rider rides a single.
    single: bool,
}

/// Add or remove a rider's single, Einzel weiblich or männlich by their
/// gender (which a new single needs).
///
/// Only the rider's club (or an admin) can change it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/set_single",
    request_body=SetSingleBody,
    responses(
        (status=200, content_type="application/json", body=SetSingleResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_single(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetSingleBody>,
) -> Result<Json<SetSingleResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.set_single(&auth.actor(), body.rider_id, body.single)
        .await?;
    Ok(Json(SetSingleResponse {}))
}
