use std::collections::HashMap;

use db_core::{DbError, FeesRepo, fees::ClubFees, ids::ClubId};

use super::SqliteRepo;

#[async_trait::async_trait]
impl FeesRepo for SqliteRepo {
    async fn fees_by_club(
        &self,
        jury_registration: bool,
    ) -> Result<HashMap<ClubId, ClubFees>, DbError> {
        Ok(crate::fees::fees(&self.pool().await, jury_registration, None).await?)
    }

    async fn fees_of_club(
        &self,
        jury_registration: bool,
        club: ClubId,
    ) -> Result<ClubFees, DbError> {
        Ok(
            crate::fees::fees(&self.pool().await, jury_registration, Some(club))
                .await?
                .remove(&club)
                .unwrap_or_default(),
        )
    }
}
