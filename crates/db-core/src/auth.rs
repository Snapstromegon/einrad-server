//! Logins, registration, email verification, password resets and the first
//! admin: the [`AuthRepo`] trait and its types.

use crate::{
    DbError,
    ids::{ClubId, UserId},
    roles::Role,
};

/// The user behind a valid session token.
#[derive(Debug, Clone)]
pub struct SessionUser {
    pub email: String,
    pub name: String,
    pub club_id: Option<ClubId>,
    pub email_verified: bool,
    /// Sorted.
    pub roles: Vec<Role>,
}

#[async_trait::async_trait]
pub trait AuthRepo {
    /// The user behind a session, with their roles.
    async fn find_session_user(&self, id: UserId) -> Result<Option<SessionUser>, DbError>;

    /// Whether a user with this email exists.
    async fn email_in_use(&self, email: &str) -> Result<bool, DbError>;

    /// Whether any user holds the admin role.
    async fn admin_exists(&self) -> Result<bool, DbError>;

    /// Creates an admin with a verified email, unless there is an admin
    /// already (then `None`). The user and the role go in together.
    async fn create_admin(
        &self,
        name: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<Option<UserId>, DbError>;
}
