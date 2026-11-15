//! Club commands: clubs, riders, pairs, acts of a club, judges, mails.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them (trimming names, requiring the gender, ...). Names are trimmed here
//! already; the commands don't trim again.

use time::OffsetDateTime;

use crate::{
    actor::Actor,
    error::CommandError,
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
};

/// Registers a rider with the club, without starts.
#[derive(Debug, Clone)]
pub struct AddRider {
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    pub gender: Gender,
}

/// Changes a rider's own data.
#[derive(Debug, Clone)]
pub struct EditRider {
    pub rider_id: RiderId,
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    /// Missing keeps the stored gender.
    pub gender: Option<Gender>,
}

#[derive(Debug, Clone)]
pub struct DeleteRider {
    pub rider_id: RiderId,
}

/// Adds or removes the rider's single.
#[derive(Debug, Clone)]
pub struct SetSingle {
    pub rider_id: RiderId,
    pub single: bool,
}

/// The rider waits for a pair partner, named or not.
#[derive(Debug, Clone)]
pub struct RequestPair {
    pub rider_id: RiderId,
    pub partner_name: Option<String>,
}

/// Links two riders to a pair act.
#[derive(Debug, Clone)]
pub struct LinkPair {
    pub rider_id: RiderId,
    pub partner_id: RiderId,
}

#[derive(Debug, Clone)]
pub struct LeavePair {
    pub rider_id: RiderId,
}

#[derive(Debug, Clone)]
pub struct DissolvePair {
    pub act_id: ActId,
}

/// Replaces a rider of a single or pair act.
#[derive(Debug, Clone)]
pub struct ReplaceActRider {
    pub act_id: ActId,
    /// The rider leaving the act.
    pub rider_id: RiderId,
    /// The rider taking their place.
    pub new_rider_id: RiderId,
}

/// Registers a club owned by the actor, who is assigned to it.
#[derive(Debug, Clone)]
pub struct CreateClub {
    pub name: String,
}

/// Renames a club. The organizer renames every club, a user only the club
/// they own (nothing changes otherwise, and it is no error).
#[derive(Debug, Clone)]
pub struct RenameClub {
    pub club_id: ClubId,
    pub name: String,
}

/// Sets the club's Vereinsverantwortliche/r; both `None` clears it.
#[derive(Debug, Clone)]
pub struct SetClubResponsible {
    pub club_id: ClubId,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// Renames an act and, while the competition takes a moderation input, sets
/// its description.
#[derive(Debug, Clone)]
pub struct EditClubAct {
    pub act_id: ActId,
    pub name: String,
    /// Only stored while the competition's `moderation_input` is on.
    pub description: Option<String>,
}

/// The write side of the club area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait ClubCommands: Send + Sync {
    /// Registers a rider with the club. The club registers each name once
    /// (ignoring case and spaces).
    async fn add_rider(&self, actor: &Actor, cmd: AddRider) -> Result<RiderId, CommandError>;

    async fn edit_rider(&self, actor: &Actor, cmd: EditRider) -> Result<(), CommandError>;

    /// Deletes the rider with their single; their pair is dissolved (the
    /// partner is left waiting) and their groups keep the other riders.
    async fn delete_rider(&self, actor: &Actor, cmd: DeleteRider) -> Result<(), CommandError>;

    /// Adds or removes the rider's single. A new single needs the rider's
    /// gender.
    async fn set_single(&self, actor: &Actor, cmd: SetSingle) -> Result<(), CommandError>;

    /// The rider waits for a pair partner, named or not. Within the club, a
    /// waiting rider of that name who names this rider (or nobody) becomes
    /// the partner at once. Returns that partner.
    async fn request_pair(
        &self,
        actor: &Actor,
        cmd: RequestPair,
    ) -> Result<Option<RiderId>, CommandError>;

    /// Links two riders to a pair act; their pair requests go. Clubs only
    /// link their own riders, the organizer (`manage_registrations`) also
    /// riders of different clubs.
    async fn link_pair(&self, actor: &Actor, cmd: LinkPair) -> Result<ActId, CommandError>;

    /// Takes the rider out of the pair competition: their pair request goes,
    /// or their pair act is dissolved and the partner is left with a pair
    /// request without a name.
    async fn leave_pair(&self, actor: &Actor, cmd: LeavePair) -> Result<(), CommandError>;

    /// Dissolves a pair act: both riders leave the pair competition, nobody
    /// is left waiting.
    async fn dissolve_pair(&self, actor: &Actor, cmd: DissolvePair) -> Result<(), CommandError>;

    /// Replaces a rider of a single or pair act by `new_rider_id`, keeping the
    /// act with its title, description and music.
    async fn replace_act_rider(
        &self,
        actor: &Actor,
        cmd: ReplaceActRider,
    ) -> Result<(), CommandError>;

    /// Registers a club owned by the actor, who is assigned to it.
    async fn create_club(&self, actor: &Actor, cmd: CreateClub) -> Result<ClubId, CommandError>;

    /// Renames the club: the organizer any club, other users only a club they
    /// own (nothing changes, and no error, for a club they don't own).
    async fn rename_club(&self, actor: &Actor, cmd: RenameClub) -> Result<(), CommandError>;

    /// Sets the club's Vereinsverantwortliche/r (both `None` clears it).
    /// `NotFound` for an unknown club, `Forbidden` unless the actor manages it.
    async fn set_club_responsible(
        &self,
        actor: &Actor,
        cmd: SetClubResponsible,
    ) -> Result<(), CommandError>;

    /// Edits an act's title and, with the competition's `moderation_input`
    /// on, its description. The access check, the flag and the write share
    /// one transaction.
    async fn edit_club_act(&self, actor: &Actor, cmd: EditClubAct) -> Result<(), CommandError>;
}
