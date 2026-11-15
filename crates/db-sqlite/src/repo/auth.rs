use db_core::{
    AuthRepo, DbError,
    auth::SessionUser,
    ids::{ClubId, UserId},
    roles::Role,
};

use super::SqliteRepo;

#[async_trait::async_trait]
impl AuthRepo for SqliteRepo {
    async fn find_session_user(&self, id: UserId) -> Result<Option<SessionUser>, DbError> {
        let pool = self.pool().await;
        let Some(user) = sqlx::query!(
            r#"SELECT email, name, club_id as "club_id: ClubId", email_verified FROM users WHERE id = $1"#,
            id,
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
        Ok(Some(SessionUser {
            email: user.email,
            name: user.name,
            club_id: user.club_id,
            email_verified: user.email_verified,
            roles,
        }))
    }

    async fn email_in_use(&self, email: &str) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE email = $1"#,
            email
        )
        .fetch_one(&pool)
        .await?;
        Ok(count > 0)
    }

    async fn admin_exists(&self) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = $1"#,
            Role::Admin
        )
        .fetch_one(&pool)
        .await?;
        Ok(count > 0)
    }

    async fn create_admin(
        &self,
        name: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<Option<UserId>, DbError> {
        let mut tx = self.begin_write().await?;
        let admins = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = $1"#,
            Role::Admin
        )
        .fetch_one(&mut *tx)
        .await?;
        if admins > 0 {
            return Ok(None);
        }
        let id = UserId::new();
        sqlx::query!(
            "INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, $2, true, $3, $4)",
            id,
            email,
            name,
            password_hash
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO user_roles (user_id, role) VALUES ($1, $2)",
            id,
            Role::Admin,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(id))
    }
}
