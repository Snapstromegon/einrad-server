use crate::act_type::ActType;
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
pub struct SetClubGroupTypeResponse {
    /// Set if a rider of the group is now also in another group of the new
    /// type. The change is saved anyway, but the groups have no category until
    /// the clubs sort it out. A German message for the user.
    warning: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetClubGroupTypeBody {
    act_id: ActId,
    /// `group_small` (Kleingruppe) or `group_large` (Großgruppe).
    act_type: ActType,
}

/// Change a group's type (Kleingruppe or Großgruppe).
///
/// Only the group's owner club (or an admin) can change it. The group then
/// belongs to the other type's categories.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/set_club_group_type",
    request_body=SetClubGroupTypeBody,
    responses(
        (status=200, content_type="application/json", body=SetClubGroupTypeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_club_group_type(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetClubGroupTypeBody>,
) -> Result<Json<SetClubGroupTypeResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let warning = db
        .set_group_type(&auth.actor(), body.act_id, body.act_type)
        .await?;

    Ok(Json(SetClubGroupTypeResponse { warning }))
}
