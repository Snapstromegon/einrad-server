use db_core::{
    ClubsRepo, DbError,
    clubs::{Actor, ClubProfile, ClubRow, RegistrationError},
    ids::{ActId, ClubId, UserId},
};

use super::SqliteRepo;
use crate::registration::{act_access, club_access};

#[async_trait::async_trait]
impl ClubsRepo for SqliteRepo {
    async fn create_club(&self, owner: UserId, name: &str) -> Result<ClubId, DbError> {
        let club_id = ClubId::new();
        let mut tx = self.begin_write().await?;
        sqlx::query!(
            "INSERT INTO clubs (id, name, owner_id) VALUES ($1, $2, $3)",
            club_id,
            name,
            owner,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE users SET club_id = $1 WHERE id = $2",
            club_id,
            owner
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(club_id)
    }

    async fn rename_club(
        &self,
        club: ClubId,
        name: &str,
        only_owner: Option<UserId>,
    ) -> Result<(), DbError> {
        let pool = self.pool().await;
        match only_owner {
            None => {
                sqlx::query!("UPDATE clubs SET name = $1 WHERE id = $2", name, club)
                    .execute(&pool)
                    .await?;
            }
            Some(owner) => {
                sqlx::query!(
                    "UPDATE clubs SET name = $1 WHERE id = $2 AND owner_id = $3",
                    name,
                    club,
                    owner,
                )
                .execute(&pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn set_club_responsible(
        &self,
        actor: &Actor,
        club: ClubId,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        club_access(&mut tx, actor, club).await?;
        sqlx::query!(
            "UPDATE clubs SET responsible_name = $1, responsible_email = $2 WHERE id = $3",
            name,
            email,
            club,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn find_club(&self, id: ClubId) -> Result<Option<ClubProfile>, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"
        SELECT
            clubs.id as "id!: ClubId",
            clubs.name,
            clubs.owner_id as "owner_id: UserId",
            clubs.fee_waiver IS NOT NULL as "fee_waived!: bool",
            users.name as "owner_name?: String",
            users.email as "owner_email?: String",
            COALESCE(clubs.responsible_name, users.name) as "responsible_name?: String",
            COALESCE(clubs.responsible_email, users.email) as "responsible_email?: String"
        FROM clubs LEFT JOIN users ON users.id = clubs.owner_id
        WHERE clubs.id = $1
        "#,
            id
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|row| ClubProfile {
            id: row.id,
            name: row.name,
            owner_id: row.owner_id,
            fee_waived: row.fee_waived,
            owner_name: row.owner_name,
            owner_email: row.owner_email,
            responsible_name: row.responsible_name,
            responsible_email: row.responsible_email,
        }))
    }

    async fn list_clubs(&self) -> Result<Vec<ClubRow>, DbError> {
        let pool = self.pool().await;
        let rows = sqlx::query!(
            r#"
        SELECT
            clubs.id as "id!: ClubId",
            clubs.name,
            clubs.owner_id as "owner_id!: UserId",
            clubs.fee_waiver,
            users.name as owner_name,
            users.email as owner_email,
            users.email_verified as owner_email_verified,
            COALESCE(clubs.responsible_name, users.name) as "responsible_name!: String",
            COALESCE(clubs.responsible_email, users.email) as "responsible_email!: String"
        FROM clubs
        JOIN users ON users.id = clubs.owner_id
        ORDER BY clubs.name
        "#
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ClubRow {
                id: row.id,
                name: row.name,
                owner_id: row.owner_id,
                fee_waiver: row.fee_waiver,
                owner_name: row.owner_name,
                owner_email: row.owner_email,
                owner_email_verified: row.owner_email_verified,
                responsible_name: row.responsible_name,
                responsible_email: row.responsible_email,
            })
            .collect())
    }

    async fn ensure_club_access(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<(), RegistrationError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        club_access(&mut conn, actor, club).await
    }

    async fn ensure_act_access(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        act_access(&mut conn, actor, act).await
    }
}
