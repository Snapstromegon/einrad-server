use db_core::{
    AuthRepo, DbError,
    auth::{Credentials, NewUser, SessionUser, UserContact},
    ids::{ClubId, UserId},
    roles::Role,
};
use uuid::Uuid;

use super::SqliteRepo;

#[async_trait::async_trait]
impl AuthRepo for SqliteRepo {
    async fn find_credentials(&self, email: &str) -> Result<Option<Credentials>, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"SELECT id as "id!: UserId", password FROM users WHERE email = $1"#,
            email,
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|row| Credentials {
            id: row.id,
            password_hash: row.password,
        }))
    }

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

    async fn find_user_by_email(&self, email: &str) -> Result<Option<UserContact>, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"SELECT id as "id!: UserId", name FROM users WHERE email = $1"#,
            email
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|row| UserContact {
            id: row.id,
            name: row.name,
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

    async fn register_user(
        &self,
        user: NewUser<'_>,
        verification_token: Uuid,
    ) -> Result<(), DbError> {
        let mut tx = self.begin_write().await?;
        sqlx::query!(
            "INSERT INTO users (id, name, email, email_verified, password) VALUES ($1, $2, $3, false, $4)",
            user.id,
            user.name,
            user.email,
            user.password_hash,
        )
        .execute(&mut *tx)
        .await?;
        let now = time::OffsetDateTime::now_utc();
        sqlx::query!(
            "INSERT INTO mail_verification (id, user_id, created_at) VALUES ($1, $2, $3)",
            verification_token,
            user.id,
            now
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn replace_verification_token(
        &self,
        user_id: UserId,
        token: Uuid,
    ) -> Result<(), DbError> {
        let mut tx = self.begin_write().await?;
        sqlx::query!("DELETE FROM mail_verification WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await?;
        let now = time::OffsetDateTime::now_utc();
        sqlx::query!(
            "INSERT INTO mail_verification (id, user_id, created_at) VALUES ($1, $2, $3)",
            token,
            user_id,
            now
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn verify_email(&self, token: Uuid) -> Result<Option<UserId>, DbError> {
        let mut tx = self.begin_write().await?;
        let Some(row) = sqlx::query!(
            r#"SELECT user_id as "user_id!: UserId" FROM mail_verification WHERE id = $1"#,
            token
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        sqlx::query!(
            "UPDATE users SET email_verified = true WHERE id = $1",
            row.user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM mail_verification WHERE user_id = $1",
            row.user_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(row.user_id))
    }

    async fn replace_password_reset_token(
        &self,
        user_id: UserId,
        token: Uuid,
    ) -> Result<(), DbError> {
        let mut tx = self.begin_write().await?;
        sqlx::query!("DELETE FROM password_resets WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await?;
        let now = time::OffsetDateTime::now_utc();
        sqlx::query!(
            "INSERT INTO password_resets (id, user_id, created_at) VALUES ($1, $2, $3)",
            token,
            user_id,
            now
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn find_password_reset(&self, token: Uuid) -> Result<Option<UserId>, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"SELECT user_id as "user_id!: UserId" FROM password_resets WHERE id = $1"#,
            token
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|row| row.user_id))
    }

    async fn set_password(&self, user_id: UserId, password_hash: &str) -> Result<(), DbError> {
        let mut tx = self.begin_write().await?;
        sqlx::query!(
            "UPDATE users SET password = $1 WHERE id = $2",
            password_hash,
            user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM password_resets WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
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
