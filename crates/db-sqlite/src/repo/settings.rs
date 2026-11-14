use db_core::{DbError, SettingsRepo, settings::FeatureFlags};

use super::SqliteRepo;
use crate::competition;

#[async_trait::async_trait]
impl SettingsRepo for SqliteRepo {
    async fn event_name(&self) -> Result<String, DbError> {
        let pool = self.pool().await;
        Ok(competition::current(&pool).await?.map_or_else(
            || "Einrad-Anmeldung".to_string(),
            |competition| competition.name,
        ))
    }

    async fn feature_flags(&self) -> Result<FeatureFlags, DbError> {
        let pool = self.pool().await;
        Ok(competition::current(&pool)
            .await?
            .map_or_else(FeatureFlags::default, |competition| FeatureFlags {
                jury_registration: competition.features.jury_registration,
                moderation_input: competition.features.moderation_input,
            }))
    }

    async fn startlist_published(&self) -> Result<bool, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_scalar!(
            r#"
            SELECT startlist_published_at IS NOT NULL as "published!: bool"
            FROM competition ORDER BY start_date DESC LIMIT 1
            "#
        )
        .fetch_optional(&pool)
        .await?
        .unwrap_or(false))
    }

    async fn set_startlist_published(
        &self,
        published: bool,
        now: time::OffsetDateTime,
    ) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        let published_at = published.then_some(now);
        let result = sqlx::query!(
            "UPDATE competition SET startlist_published_at = $1",
            published_at
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }
}
