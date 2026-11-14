use crate::ids::ActId;
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
pub struct DissolvePairResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DissolvePairBody {
    act_id: ActId,
}

/// Dissolves a pair act: both riders leave the pair competition, nobody is
/// left waiting for a partner. To keep the partner waiting, use `leave_pair`.
///
/// Only the club of one of its riders (or an admin) can do it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/dissolve_pair",
    request_body=DissolvePairBody,
    responses(
        (status=200, content_type="application/json", body=DissolvePairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn dissolve_pair(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DissolvePairBody>,
) -> Result<Json<DissolvePairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.dissolve_pair(&auth.actor(), body.act_id).await?;
    Ok(Json(DissolvePairResponse {}))
}
