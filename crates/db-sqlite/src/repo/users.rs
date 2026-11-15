use db_core::{
    DbError, UsersRepo,
    ids::{ClubId, UserId},
    queries::user::User,
    roles::Role,
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
}
