use db_core::{DbError, SettingsRepo, settings::FeatureFlags};

use super::SqliteRepo;

#[async_trait::async_trait]
impl SettingsRepo for SqliteRepo {
    async fn event_name(&self) -> Result<String, DbError> {
        let pool = self.pool().await;
        let name =
            sqlx::query_scalar!("SELECT name FROM competition ORDER BY start_date DESC LIMIT 1")
                .fetch_optional(&pool)
                .await?;
        Ok(name.unwrap_or_else(|| "Einrad-Anmeldung".to_string()))
    }

    async fn feature_flags(&self) -> Result<FeatureFlags, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"
            SELECT
                feature_jury_registration as "feature_jury_registration: bool",
                feature_moderation_input as "feature_moderation_input: bool"
            FROM competition ORDER BY start_date DESC LIMIT 1
            "#
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map_or_else(FeatureFlags::default, |row| FeatureFlags {
            jury_registration: row.feature_jury_registration,
            moderation_input: row.feature_moderation_input,
        }))
    }
}
