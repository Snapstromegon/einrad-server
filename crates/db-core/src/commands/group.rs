//! Group commands: groups (acts of several riders).

use crate::{
    act_type::ActType,
    actor::Actor,
    error::CommandError,
    ids::{ActId, ClubId, RiderId},
};

/// A rider a club sets in a group.
#[derive(Debug, Clone, Copy)]
pub struct GroupMember {
    pub rider_id: RiderId,
    /// Replacement riders (Ersatzfahrer) don't count towards the group size.
    pub is_replacement: bool,
}

/// Creates a group act for the club, which becomes its owner.
#[derive(Debug, Clone)]
pub struct CreateClubGroup {
    pub club_id: ClubId,
    pub act_type: ActType,
    /// Normalized by the command (whitespace, forbidden characters, length).
    pub group_name: String,
}

#[derive(Debug, Clone)]
pub struct RenameClubGroup {
    pub act_id: ActId,
    pub group_name: String,
}

#[derive(Debug, Clone)]
pub struct DeleteClubGroup {
    pub act_id: ActId,
}

/// Makes `members` the club's riders in the group.
#[derive(Debug, Clone)]
pub struct SetClubGroupMembers {
    pub club_id: ClubId,
    pub act_id: ActId,
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone)]
pub struct SetClubGroupType {
    pub act_id: ActId,
    pub act_type: ActType,
}

/// The write side of the group area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait GroupCommands: Send + Sync {
    /// Creates a group act for the club, which becomes its owner. Group
    /// names are unique across the competition (case-insensitive).
    async fn create_club_group(
        &self,
        actor: &Actor,
        cmd: CreateClubGroup,
    ) -> Result<ActId, CommandError>;

    /// Only the owner club (or the organizer) renames a group.
    async fn rename_club_group(
        &self,
        actor: &Actor,
        cmd: RenameClubGroup,
    ) -> Result<(), CommandError>;

    /// Deletes a group; its riders stay registered. The owner club can't
    /// delete it while riders of other clubs are in it.
    async fn delete_club_group(
        &self,
        actor: &Actor,
        cmd: DeleteClubGroup,
    ) -> Result<(), CommandError>;

    /// Makes `members` the club's riders in the group; riders of other clubs
    /// are kept. Returns the type conflict warning, if any: a rider can be in
    /// at most one small and one large group, replacements included. The
    /// rule isn't enforced when riders change (clubs add their riders to a
    /// shared group one after another); groups breaking it get no category
    /// and the clubs this German message naming the riders, their clubs and
    /// the other groups.
    async fn set_club_group_members(
        &self,
        actor: &Actor,
        cmd: SetClubGroupMembers,
    ) -> Result<Option<String>, CommandError>;

    /// Changes a group's type. Returns the type conflict warning, if any
    /// (see [`GroupCommands::set_club_group_members`]).
    async fn set_club_group_type(
        &self,
        actor: &Actor,
        cmd: SetClubGroupType,
    ) -> Result<Option<String>, CommandError>;
}
