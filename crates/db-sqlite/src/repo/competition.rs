use db_core::{CompetitionRepo, DbError, competition::Competition};

use super::SqliteRepo;
use crate::competition;

#[async_trait::async_trait]
impl CompetitionRepo for SqliteRepo {
    async fn current_competition(&self) -> Result<Option<Competition>, DbError> {
        let pool = self.pool().await;
        Ok(competition::current(&pool).await?)
    }
}
