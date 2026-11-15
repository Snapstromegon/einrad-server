use db_core::{DbError, GroupsRepo, ids::ActId};

use super::SqliteRepo;
use crate::group_rules::type_conflict_message;

#[async_trait::async_trait]
impl GroupsRepo for SqliteRepo {
    async fn group_conflict(&self, act: ActId) -> Result<Option<String>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(type_conflict_message(&mut conn, act).await?)
    }
}
