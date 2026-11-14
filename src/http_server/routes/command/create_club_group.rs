use crate::act_type::ActType;
use crate::ids::{ActId, ClubId};
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
pub struct CreateClubGroupResponse {
    act_id: ActId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClubGroupBody {
    club_id: ClubId,
    group_name: String,
    /// `group_small` (Kleingruppe) or `group_large` (Großgruppe). The type
    /// stays as chosen; the number of riders doesn't change it.
    act_type: ActType,
}

/// Create a group act.
///
/// The club becomes the group's owner and chooses its type. The group starts
/// without riders; add them with `set_club_group_members`. Group names are
/// unique across the competition (case-insensitive).
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/create_club_group",
    request_body=CreateClubGroupBody,
    responses(
        (status=200, content_type="application/json", body=CreateClubGroupResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn create_club_group(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<CreateClubGroupBody>,
) -> Result<Json<CreateClubGroupResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let act_id = db
        .create_group(&auth.actor(), body.club_id, body.act_type, &body.group_name)
        .await?;

    Ok(Json(CreateClubGroupResponse { act_id }))
}
