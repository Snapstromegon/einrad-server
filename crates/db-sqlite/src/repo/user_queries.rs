//! `UserQueries` for SQLite.

use std::collections::HashMap;

use db_core::{
    actor::Actor,
    error::QueryError,
    ids::{ClubId, UserId},
    queries::user::{User, UserQueries},
    roles::Role,
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};

#[async_trait::async_trait]
impl UserQueries for SqliteRepo {
    async fn list_users(&self, _actor: &Actor) -> Result<Vec<User>, QueryError> {
        read_tx!(self, tx, { Ok(list_users_in(&mut tx).await?) })
    }
}

async fn list_users_in(conn: &mut SqliteConnection) -> Result<Vec<User>, sqlx::Error> {
    let role_rows = sqlx::query!(
        r#"SELECT user_id as "user_id: UserId", role as "role: Role" FROM user_roles"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut roles: HashMap<UserId, Vec<Role>> = HashMap::new();
    for row in role_rows {
        roles.entry(row.user_id).or_default().push(row.role);
    }
    for user_roles in roles.values_mut() {
        user_roles.sort_unstable();
    }
    let users = sqlx::query!(
        r#"SELECT id as "id!: UserId", club_id as "club_id: ClubId", name, email, email_verified FROM users"#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| User {
        roles: roles.remove(&row.id).unwrap_or_default(),
        id: row.id,
        name: row.name,
        email: row.email,
        email_verified: row.email_verified,
        club_id: row.club_id,
    })
    .collect();
    Ok(users)
}
