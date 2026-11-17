use std::collections::HashSet;

use db_core::{CheckInRepo, DbError, check_in::CheckIn, clubs::RegistrationError, ids::RiderId};
use time::OffsetDateTime;

use super::SqliteRepo;

#[async_trait::async_trait]
impl CheckInRepo for SqliteRepo {
    async fn set_checked_in(
        &self,
        riders: &[RiderId],
        checked_in_at: Option<OffsetDateTime>,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let riders: HashSet<RiderId> = riders.iter().copied().collect();
        for rider_id in riders {
            let result = if let Some(checked_in_at) = checked_in_at {
                sqlx::query!(
                    "UPDATE rider SET checked_in_at = COALESCE(checked_in_at, $1) WHERE id = $2",
                    checked_in_at,
                    rider_id,
                )
                .execute(&mut *tx)
                .await?
            } else {
                sqlx::query!(
                    "UPDATE rider SET checked_in_at = NULL WHERE id = $1",
                    rider_id
                )
                .execute(&mut *tx)
                .await?
            };
            if result.rows_affected() == 0 {
                return Err(RegistrationError::NotFound);
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_check_ins(&self) -> Result<Vec<CheckIn>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_as!(
            CheckIn,
            r#"
            SELECT
                id as "rider_id!: RiderId",
                checked_in_at as "checked_in_at: OffsetDateTime"
            FROM rider
            "#
        )
        .fetch_all(&pool)
        .await?)
    }
}
