use crate::ids::{ActId, RiderId};
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
pub struct LinkPairResponse {
    act_id: ActId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkPairBody {
    rider_id: RiderId,
    partner_id: RiderId,
}

/// Links two riders to a pair act; their pair requests are fulfilled.
///
/// Clubs link riders of their own club, the organizer also riders of
/// different clubs. Fails if one of them already rides a pair.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/link_pair",
    request_body=LinkPairBody,
    responses(
        (status=200, content_type="application/json", body=LinkPairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn link_pair(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<LinkPairBody>,
) -> Result<Json<LinkPairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let act_id = db
        .link_pair(&auth.actor(), body.rider_id, body.partner_id)
        .await?;
    Ok(Json(LinkPairResponse { act_id }))
}
