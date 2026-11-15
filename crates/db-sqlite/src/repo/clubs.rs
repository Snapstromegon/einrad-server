use db_core::{
    ClubsRepo,
    clubs::{Actor, RegistrationError},
    ids::{ActId, ClubId},
};

use super::SqliteRepo;
use crate::registration::{act_access, club_access};

#[async_trait::async_trait]
impl ClubsRepo for SqliteRepo {
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
