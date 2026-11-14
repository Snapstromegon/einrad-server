//! Clubs: the [`ClubsRepo`] trait, the [`Actor`] who asks and the
//! [`RegistrationError`] the club-side commands fail with.

use crate::{
    DbError,
    ids::{ActId, ClubId, UserId},
};

/// Who asks, as far as club access is concerned. Built from the logged-in
/// user by the HTTP layer.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub user_id: UserId,
    /// The club the user is assigned to.
    pub club_id: Option<ClubId>,
    /// The organizer may manage every club.
    pub manage_registrations: bool,
}

impl Actor {
    /// Whether the actor may manage the club owned by `owner_id`: the
    /// organizer, the club's owner and users assigned to the club.
    #[must_use]
    pub fn may_manage_club(&self, club_id: ClubId, owner_id: UserId) -> bool {
        self.manage_registrations || owner_id == self.user_id || self.club_id == Some(club_id)
    }
}

/// Why a club-side command or query failed. `Rejected` carries a German
/// message for the user.
#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    #[error("Not found")]
    NotFound,
    #[error("Forbidden")]
    Forbidden,
    #[error("{0}")]
    Rejected(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<sqlx::Error> for RegistrationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error.into())
    }
}

/// A club with its owner and contact.
#[derive(Debug, Clone)]
pub struct ClubProfile {
    pub id: ClubId,
    pub name: String,
    pub owner_id: UserId,
    pub fee_waived: bool,
    pub owner_name: Option<String>,
    pub owner_email: Option<String>,
    /// The club's own responsible person, else the owner.
    pub responsible_name: Option<String>,
    pub responsible_email: Option<String>,
}

/// A row of the club list.
#[derive(Debug, Clone)]
pub struct ClubRow {
    pub id: ClubId,
    pub name: String,
    pub owner_id: UserId,
    pub fee_waiver: Option<String>,
    pub owner_name: String,
    pub owner_email: String,
    pub owner_email_verified: bool,
    pub responsible_name: String,
    pub responsible_email: String,
}

#[async_trait::async_trait]
pub trait ClubsRepo {
    /// Registers a club owned by the user, who is assigned to it.
    async fn create_club(&self, owner: UserId, name: &str) -> Result<ClubId, DbError>;

    /// Renames the club. With `only_owner`, only if that user owns it
    /// (nothing changes otherwise).
    async fn rename_club(
        &self,
        club: ClubId,
        name: &str,
        only_owner: Option<UserId>,
    ) -> Result<(), DbError>;

    /// Sets the club's Vereinsverantwortliche/r (both `None` clears it).
    async fn set_club_responsible(
        &self,
        actor: &Actor,
        club: ClubId,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Result<(), RegistrationError>;

    async fn find_club(&self, id: ClubId) -> Result<Option<ClubProfile>, DbError>;

    /// All clubs by name.
    async fn list_clubs(&self) -> Result<Vec<ClubRow>, DbError>;

    /// Fails with `NotFound` for an unknown club and `Forbidden` unless the
    /// actor may manage it: the organizer, the owner and users assigned to
    /// the club.
    async fn ensure_club_access(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<(), RegistrationError>;

    /// Fails unless the actor may change the act: the organizer and users
    /// with access to the club of one of its riders or, for groups, to the
    /// club that created it.
    async fn ensure_act_access(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError>;
}
