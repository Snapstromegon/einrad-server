//! The first admin: inserted from `[admin]`/`ADMIN` at startup, or by the
//! first-run setup (ADR 0011).

use crate::ids::UserId;
use db_core::{Db, DbError};
use password_auth::generate_hash;

/// Whether the database has an admin.
///
/// # Errors
/// If the database can't be read.
pub async fn admin_exists(db: &dyn Db) -> Result<bool, DbError> {
    db.admin_exists().await
}

/// Whether a user with this email exists.
///
/// # Errors
/// If the database can't be read.
pub async fn email_in_use(db: &dyn Db, email: &str) -> Result<bool, DbError> {
    db.email_in_use(email).await
}

/// Inserts an admin with a verified email, unless the database already has
/// one. Returns whether it inserted one.
///
/// # Errors
/// If the database can't be written.
pub async fn insert_admin_user(
    db: &dyn Db,
    name: &str,
    email: &str,
    password: &str,
) -> Result<bool, DbError> {
    if db.admin_exists().await? {
        return Ok(false);
    }
    insert_admin_with_hash(db, name, email, &generate_hash(password)).await
}

/// Like [`insert_admin_user`] with a password that's already hashed. The user
/// and the role go in together, so a failure leaves no half-made admin.
///
/// # Errors
/// If the database can't be written, e.g. the email is taken.
pub async fn insert_admin_with_hash(
    db: &dyn Db,
    name: &str,
    email: &str,
    hash: &str,
) -> Result<bool, DbError> {
    Ok(create_admin_with_hash(db, name, email, hash)
        .await?
        .is_some())
}

/// Like [`insert_admin_with_hash`], returning the new admin's id, or `None`
/// if the database already has an admin.
///
/// # Errors
/// If the database can't be written, e.g. the email is taken.
pub async fn create_admin_with_hash(
    db: &dyn Db,
    name: &str,
    email: &str,
    hash: &str,
) -> Result<Option<UserId>, DbError> {
    db.create_admin(name, email, hash).await
}
