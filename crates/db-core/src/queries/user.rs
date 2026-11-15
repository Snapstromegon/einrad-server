//! User queries: users, login and registration.

use crate::{
    actor::Actor,
    error::QueryError,
    ids::{ClubId, UserId},
    roles::Role,
};

/// A user as the organizers see them.
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub email: String,
    pub email_verified: bool,
    /// Sorted.
    pub roles: Vec<Role>,
    pub club_id: Option<ClubId>,
}

/// The read side of the user area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait UserQueries: Send + Sync {
    /// Every user with their roles, in one snapshot.
    async fn list_users(&self, actor: &Actor) -> Result<Vec<User>, QueryError>;
}
