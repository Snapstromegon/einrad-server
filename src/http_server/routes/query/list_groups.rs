use crate::act_type::ActType;
use crate::ids::{ActId, CategoryId, ClubId};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::group as read};
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct GroupSummary {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders).
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub owner_club_name: Option<String>,
    pub regular_count: i64,
    pub replacement_count: i64,
    /// The clubs with riders in the group, sorted by name.
    pub club_names: Vec<String>,
}

impl From<read::GroupSummary> for GroupSummary {
    fn from(group: read::GroupSummary) -> Self {
        Self {
            id: group.id,
            group_name: group.group_name,
            act_type: group.act_type,
            category_id: group.category_id,
            category: group.category,
            owner_club_id: group.owner_club_id,
            owner_club_name: group.owner_club_name,
            regular_count: group.regular_count,
            replacement_count: group.replacement_count,
            club_names: group.club_names,
        }
    }
}

/// List all groups.
///
/// Lets clubs pick an existing group to join. Contains no rider data.
#[utoipa::path(
    get,
    tags=["query", "group"],
    path="/list_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<GroupSummary>),
        (status=401, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_groups(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
) -> Result<Json<Vec<GroupSummary>>, HttpError> {
    let groups = db.list_groups(&auth.actor()).await?;
    Ok(Json(groups.into_iter().map(Into::into).collect()))
}
