use db_core::{CompetitionRepo, DbError, branding::Branding, competition::Competition};

use super::SqliteRepo;
use crate::competition;

#[async_trait::async_trait]
impl CompetitionRepo for SqliteRepo {
    async fn current_competition(&self) -> Result<Option<Competition>, DbError> {
        let pool = self.pool().await;
        Ok(competition::current(&pool).await?)
    }

    async fn save_competition_settings(&self, edited: &Competition) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        match competition::current_id(&mut *tx).await? {
            Some(id) if id == edited.id => {}
            _ => return Ok(false),
        }
        competition::upsert(&mut *tx, edited).await?;
        // The form sets both colors; `None` is the default (ADR 0009).
        competition::write_colors(
            &mut *tx,
            &edited.id,
            &Branding::set_or_reset(edited.primary_color.clone()),
            &Branding::set_or_reset(edited.secondary_color.clone()),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}
