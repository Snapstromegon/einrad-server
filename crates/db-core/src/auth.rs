//! Logins, registration, email verification, password resets and the first
//! admin: the [`AuthRepo`] trait and its types.

use uuid::Uuid;

use crate::{
    DbError,
    ids::{ClubId, UserId},
    roles::Role,
};

/// What a login checks.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub id: UserId,
    /// The stored password hash.
    pub password_hash: String,
}

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

/// A user found by email, e.g. to mail them a link.
#[derive(Debug, Clone)]
pub struct UserContact {
    pub id: UserId,
    pub name: String,
}

/// A user to create. The email is not verified yet.
#[derive(Debug, Clone)]
pub struct NewUser<'a> {
    pub id: UserId,
    pub name: &'a str,
    pub email: &'a str,
    pub password_hash: &'a str,
}

#[async_trait::async_trait]
pub trait AuthRepo {
    /// The login data of the user with this email.
    async fn find_credentials(&self, email: &str) -> Result<Option<Credentials>, DbError>;

    /// The user behind a session, with their roles.
    async fn find_session_user(&self, id: UserId) -> Result<Option<SessionUser>, DbError>;

    /// The user with this email.
    async fn find_user_by_email(&self, email: &str) -> Result<Option<UserContact>, DbError>;

    /// Whether a user with this email exists.
    async fn email_in_use(&self, email: &str) -> Result<bool, DbError>;

    /// Creates a user with an unverified email together with the token that
    /// verifies it.
    async fn register_user(
        &self,
        user: NewUser<'_>,
        verification_token: Uuid,
    ) -> Result<(), DbError>;

    /// Replaces the user's open verification tokens with `token`.
    async fn replace_verification_token(&self, user_id: UserId, token: Uuid)
    -> Result<(), DbError>;

    /// Marks the email behind the token as verified and drops the user's
    /// tokens. Returns the user, or `None` for an unknown token.
    async fn verify_email(&self, token: Uuid) -> Result<Option<UserId>, DbError>;

    /// Replaces the user's open password reset tokens with `token`.
    async fn replace_password_reset_token(
        &self,
        user_id: UserId,
        token: Uuid,
    ) -> Result<(), DbError>;

    /// The user a password reset token belongs to.
    async fn find_password_reset(&self, token: Uuid) -> Result<Option<UserId>, DbError>;

    /// Sets a new password hash and drops the user's password reset tokens.
    async fn set_password(&self, user_id: UserId, password_hash: &str) -> Result<(), DbError>;

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
