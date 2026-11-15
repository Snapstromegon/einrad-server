//! Who asks: the one [`Actor`] every authenticated command and query takes.

use crate::{
    ids::{ClubId, UserId},
    roles::{self, Permission, Role},
};

/// The logged-in user behind a command or query, as the HTTP layer builds it
/// from the session. It replaces the former `clubs::Actor` (club access) and
/// `bookings::Actor` (payment log).
///
/// Role gates stay in the server's `Granted<permit::X>` extractor; the actor
/// carries what data-dependent access and the payment log need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: UserId,
    /// Shown in the payment log.
    pub name: String,
    /// Sorted, as on the user.
    pub roles: Vec<Role>,
    /// The club the user is assigned to.
    pub club_id: Option<ClubId>,
}

impl Actor {
    #[must_use]
    pub fn new(
        user_id: UserId,
        name: impl Into<String>,
        roles: Vec<Role>,
        club_id: Option<ClubId>,
    ) -> Self {
        Self {
            user_id,
            name: name.into(),
            roles,
            club_id,
        }
    }

    /// Whether one of the actor's roles grants `permission`.
    #[must_use]
    pub fn can(&self, permission: Permission) -> bool {
        self.roles
            .iter()
            .any(|r| r.permissions().contains(&permission))
    }

    /// The organizer may manage every club.
    #[must_use]
    pub fn manage_registrations(&self) -> bool {
        self.can(Permission::ManageRegistrations)
    }

    /// All permissions of the actor's roles, sorted.
    #[must_use]
    pub fn permissions(&self) -> Vec<Permission> {
        roles::permissions_of(&self.roles)
    }
}
