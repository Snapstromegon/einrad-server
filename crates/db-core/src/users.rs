//! Users and their roles: the [`UsersRepo`] trait.

use crate::{DbError, ids::UserId, queries::user::User};

#[async_trait::async_trait]
pub trait UsersRepo {
    /// One user with their roles.
    async fn find_user(&self, id: UserId) -> Result<Option<User>, DbError>;
}
