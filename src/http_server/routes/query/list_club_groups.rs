use crate::act_type::ActType;
use crate::ids::{ActId, CategoryId, ClubId, RiderId};
use axum::{Extension, Json, extract::Query};
use db_core::{prelude::*, queries::group as read};
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

/// A group with its members, as a club sees it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroup {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders) and groups
    /// with a `conflict`.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Age in whole years of the oldest rider (replacements included) on the
    /// determining day.
    pub max_age: Option<f64>,
    /// Set if a rider of the group is also in another group of the same type:
    /// a German message naming them. The group has no category until that's
    /// sorted out.
    pub conflict: Option<String>,
    /// The act's title.
    pub name: String,
    pub song_file: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub owner_club_name: Option<String>,
    /// Whether the queried club created the group (and may rename or delete it).
    pub is_owner: bool,
    /// Regular riders first, then replacements, each sorted by name.
    pub members: Vec<ClubGroupMember>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroupMember {
    pub rider_id: RiderId,
    pub firstname: String,
    pub lastname: String,
    pub club_id: ClubId,
    pub club_name: String,
    pub is_replacement: bool,
    /// Only for the queried club's own riders.
    #[serde(with = "time::serde::iso8601::option")]
    pub birthdate: Option<time::OffsetDateTime>,
}

impl From<read::ClubGroup> for ClubGroup {
    fn from(group: read::ClubGroup) -> Self {
        Self {
            id: group.id,
            group_name: group.group_name,
            act_type: group.act_type,
            category_id: group.category_id,
            category: group.category,
            max_age: group.max_age,
            conflict: group.conflict,
            name: group.name,
            song_file: group.song_file,
            owner_club_id: group.owner_club_id,
            owner_club_name: group.owner_club_name,
            is_owner: group.is_owner,
            members: group.members.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<read::ClubGroupMember> for ClubGroupMember {
    fn from(member: read::ClubGroupMember) -> Self {
        Self {
            rider_id: member.rider_id,
            firstname: member.firstname,
            lastname: member.lastname,
            club_id: member.club_id,
            club_name: member.club_name,
            is_replacement: member.is_replacement,
            birthdate: member.birthdate,
        }
    }
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubGroupsQuery {
    club_id: ClubId,
}

/// List the groups of a club.
///
/// Groups the club created or has riders in, with all their members.
#[utoipa::path(
    get,
    tags=["query", "club", "group"],
    params(ListClubGroupsQuery),
    path="/list_club_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubGroup>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_groups(
    Extension(db): Extension<DbHandle>,
    Query(query): Query<ListClubGroupsQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubGroup>>, HttpError> {
    let groups = db
        .list_club_groups(
            &auth.actor(),
            read::ListClubGroups {
                club_id: query.club_id,
            },
        )
        .await?;
    Ok(Json(groups.into_iter().map(Into::into).collect()))
}
