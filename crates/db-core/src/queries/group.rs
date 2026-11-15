//! Group queries: groups (acts of several riders).
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use crate::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{ActId, CategoryId, ClubId, RiderId},
};

/// The groups of one club.
#[derive(Debug, Clone)]
pub struct ListClubGroups {
    pub club_id: ClubId,
}

/// A group with its members, as a club sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubGroup {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `None` for incomplete groups (fewer than 3 regular riders) and groups
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

#[derive(Debug, Clone, PartialEq)]
pub struct ClubGroupMember {
    pub rider_id: RiderId,
    pub firstname: String,
    pub lastname: String,
    pub club_id: ClubId,
    pub club_name: String,
    pub is_replacement: bool,
    /// Only for the queried club's own riders.
    pub birthdate: Option<time::OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupSummary {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `None` for incomplete groups (fewer than 3 regular riders).
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub owner_club_name: Option<String>,
    pub regular_count: i64,
    pub replacement_count: i64,
    /// The clubs with riders in the group, sorted by name.
    pub club_names: Vec<String>,
}

/// The read side of the group area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait GroupQueries: Send + Sync {
    /// The groups the club created or has riders in, with all their members,
    /// by name. `NotFound` for an unknown club, `Forbidden` unless the actor
    /// manages it.
    async fn list_club_groups(
        &self,
        actor: &Actor,
        query: ListClubGroups,
    ) -> Result<Vec<ClubGroup>, QueryError>;

    /// All groups, without rider data, by name.
    async fn list_groups(&self, actor: &Actor) -> Result<Vec<GroupSummary>, QueryError>;
}
