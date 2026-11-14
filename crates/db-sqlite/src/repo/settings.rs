use db_core::{DbError, SettingsRepo};

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
}
