//! Users and their roles: the [`UsersRepo`] trait and its types.

use crate::{
    DbError,
    ids::{ClubId, UserId},
    roles::Role,
};

/// A user as the organizers see them.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub email: String,
    pub email_verified: bool,
    /// Sorted.
    pub roles: Vec<Role>,
    pub club_id: Option<ClubId>,
}

#[async_trait::async_trait]
pub trait UsersRepo {
    /// One user with their roles.
    async fn find_user(&self, id: UserId) -> Result<Option<User>, DbError>;

    /// Every user with their roles.
    async fn list_users(&self) -> Result<Vec<User>, DbError>;

    /// Makes `roles` exactly the roles of the user, in one unit of work.
    /// Returns `false` (and changes nothing) if there is no such user.
    async fn set_roles(&self, user_id: UserId, roles: &[Role]) -> Result<bool, DbError>;
}
