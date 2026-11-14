use std::collections::HashMap;

use db_core::{
    DbError, UsersRepo,
    ids::{ClubId, UserId},
    roles::Role,
    users::User,
};

use super::SqliteRepo;

#[async_trait::async_trait]
impl UsersRepo for SqliteRepo {
    async fn find_user(&self, id: UserId) -> Result<Option<User>, DbError> {
        let pool = self.pool().await;
        let Some(row) = sqlx::query!(
            r#"SELECT id as "id!: UserId", name, email, email_verified, club_id as "club_id: ClubId" FROM users WHERE id = $1"#,
            id
        )
        .fetch_optional(&pool)
        .await?
        else {
            return Ok(None);
        };
        let mut roles = sqlx::query_scalar!(
            r#"SELECT role as "role: Role" FROM user_roles WHERE user_id = $1"#,
            id
        )
        .fetch_all(&pool)
        .await?;
        roles.sort_unstable();
        Ok(Some(User {
            id: row.id,
            name: row.name,
            email: row.email,
            email_verified: row.email_verified,
            roles,
            club_id: row.club_id,
        }))
    }

    async fn list_users(&self) -> Result<Vec<User>, DbError> {
        let pool = self.pool().await;
        let role_rows = sqlx::query!(
            r#"SELECT user_id as "user_id: UserId", role as "role: Role" FROM user_roles"#
        )
        .fetch_all(&pool)
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
        .fetch_all(&pool)
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

    async fn set_roles(&self, user_id: UserId, roles: &[Role]) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        let exists = sqlx::query_scalar!("SELECT 1 FROM users WHERE id = $1", user_id)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_none() {
            return Ok(false);
        }
        sqlx::query!("DELETE FROM user_roles WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await?;
        for role in roles {
            sqlx::query!(
                "INSERT INTO user_roles (user_id, role) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                user_id,
                role
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}
