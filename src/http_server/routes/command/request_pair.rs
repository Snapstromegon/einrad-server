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
pub struct RequestPairResponse {
    /// The waiting rider of the club the request was matched with, now the
    /// partner of a pair act.
    partner_id: Option<RiderId>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestPairBody {
    rider_id: RiderId,
    /// The partner the club enters, if it knows them.
    partner_name: Option<String>,
}

/// The rider waits for a pair partner (a pair request, Paaranfrage), named
/// or not; it replaces an earlier request. A waiting rider of the club with
/// that name, who names this rider or nobody, becomes the partner at once.
/// Partners of other clubs are linked by the organizer.
///
/// Fails if the rider already rides a pair. Only the rider's club (or an
/// admin) can ask.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/request_pair",
    request_body=RequestPairBody,
    responses(
        (status=200, content_type="application/json", body=RequestPairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn request_pair(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<RequestPairBody>,
) -> Result<Json<RequestPairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let partner_id = db
        .request_pair(&auth.actor(), body.rider_id, body.partner_name.as_deref())
        .await?;
    Ok(Json(RequestPairResponse { partner_id }))
}
