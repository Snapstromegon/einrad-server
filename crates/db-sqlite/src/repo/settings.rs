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
}
