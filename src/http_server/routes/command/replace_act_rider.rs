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
pub struct ReplaceActRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReplaceActRiderBody {
    act_id: ActId,
    /// The rider leaving the act.
    rider_id: RiderId,
    /// The rider taking their place, of the same club.
    new_rider_id: RiderId,
}

/// Replace a rider of a single or pair, keeping the act with its title,
/// description and music.
///
/// For a single, the new rider gets the single (by their gender, which must
/// be known) and the old one loses it. For a pair, the new rider becomes the
/// partner of the remaining rider (and their pair request is fulfilled). The
/// new rider must be of the same club (admins: any club) and not already ride
/// a single or pair. Group riders
/// are set with `set_club_group_members`.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/replace_act_rider",
    request_body=ReplaceActRiderBody,
    responses(
        (status=200, content_type="application/json", body=ReplaceActRiderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn replace_act_rider(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<ReplaceActRiderBody>,
) -> Result<Json<ReplaceActRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.replace_act_rider(&auth.actor(), body.act_id, body.rider_id, body.new_rider_id)
        .await?;
    Ok(Json(ReplaceActRiderResponse {}))
}
