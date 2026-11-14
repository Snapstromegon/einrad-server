use crate::ids::{ActId, ClubId, RiderId};

use axum::{Extension, Json, http::StatusCode};
use db_core::{DbHandle, groups::GroupMember};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetClubGroupMembersResponse {
    /// Set if a rider of the group is now also in another group of the same
    /// type. The change is saved anyway, but the group has no category until
    /// the clubs sort it out. A German message for the user.
    warning: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ClubGroupMemberInput {
    rider_id: RiderId,
    /// Replacement riders (Ersatzfahrer) don't count towards the group size.
    is_replacement: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetClubGroupMembersBody {
    club_id: ClubId,
    act_id: ActId,
    /// All riders of this club in the group. Riders of other clubs are kept.
    /// An empty list means the club leaves the group.
    members: Vec<ClubGroupMemberInput>,
}

/// Set a club's riders in a group.
///
/// Every club can join every group, but only with its own riders. The group's
/// type stays as its owner chose it, whatever the number of riders. If a rider
/// of the group (from any club) is then in two groups of the same type, the
/// change is saved with a warning and both groups lose their category until
/// that's sorted out, since clubs add their riders to a shared group one after
/// another.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/set_club_group_members",
    request_body=SetClubGroupMembersBody,
    responses(
        (status=200, content_type="application/json", body=SetClubGroupMembersResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_club_group_members(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetClubGroupMembersBody>,
) -> Result<Json<SetClubGroupMembersResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let members: Vec<GroupMember> = body
        .members
        .iter()
        .map(|member| GroupMember {
            rider_id: member.rider_id,
            is_replacement: member.is_replacement,
        })
        .collect();
    let warning = db
        .set_group_members(&auth.actor(), body.club_id, body.act_id, &members)
        .await?;

    Ok(Json(SetClubGroupMembersResponse { warning }))
}
