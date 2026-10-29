use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct LeavePairResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LeavePairBody {
    rider_id: Uuid,
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
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<LeavePairBody>,
) -> Result<Json<LeavePairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_rider_access(&mut transaction, &auth, body.rider_id).await?;
    rider_registration::leave_pair(&mut transaction, body.rider_id).await?;
    transaction.commit().await?;
    Ok(Json(LeavePairResponse {}))
}
