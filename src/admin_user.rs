//! The first admin: inserted from `[admin]`/`ADMIN` at startup, or by the
//! first-run setup (ADR 0011).

use password_auth::generate_hash;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::roles::Role;

/// Whether the database has an admin.
///
/// # Errors
/// If the database can't be read.
pub async fn admin_exists(db: &SqlitePool) -> sqlx::Result<bool> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = ?"#,
        Role::Admin
    )
    .fetch_one(db)
    .await?;
    Ok(count > 0)
}

/// Whether a user with this email exists.
///
/// # Errors
/// If the database can't be read.
pub async fn email_in_use(db: &SqlitePool, email: &str) -> sqlx::Result<bool> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE email = ?"#,
        email
    )
    .fetch_one(db)
    .await?;
    Ok(count > 0)
}

/// Inserts an admin with a verified email, unless the database already has
/// one. Returns whether it inserted one.
///
/// # Errors
/// If the database can't be written.
pub async fn insert_admin_user(
    db: &SqlitePool,
    name: &str,
    email: &str,
    password: &str,
) -> sqlx::Result<bool> {
    if admin_exists(db).await? {
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
    db: &SqlitePool,
    name: &str,
    email: &str,
    hash: &str,
) -> sqlx::Result<bool> {
    let mut tx = db.begin().await?;
    let admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = ?"#,
        Role::Admin
    )
    .fetch_one(&mut *tx)
    .await?;
    if admins > 0 {
        return Ok(false);
    }
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO users (id, email, email_verified, name, password) VALUES (?, ?, true, ?, ?)",
        id,
        email,
        name,
        hash
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO user_roles (user_id, role) VALUES (?, ?)",
        id,
        Role::Admin,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}
