//! The leftover of `RidersRepo` (#238): the rider starts, read for the demo
//! seed and tests. Moves to `Support` with the demo seed.

use db_core::{DbError, RidersRepo, ids::RiderId, riders::Starts};

use super::SqliteRepo;
use crate::registration;

#[async_trait::async_trait]
impl RidersRepo for SqliteRepo {
    async fn rider_starts(&self, rider: RiderId) -> Result<Starts, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(registration::starts(&mut conn, rider).await?)
    }
}
