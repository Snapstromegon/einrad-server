use db_core::{
    DbError, MediaRepo,
    media::{Image, Slot, StoredLogo},
};
use time::OffsetDateTime;

use super::SqliteRepo;
use crate::images;

#[async_trait::async_trait]
impl MediaRepo for SqliteRepo {
    async fn list_images(&self) -> Result<Vec<Image>, DbError> {
        let pool = self.pool().await;
        Ok(images::list(&pool).await?)
    }

    async fn image_meta(&self, slot: Slot) -> Result<Option<(i64, OffsetDateTime)>, DbError> {
        let pool = self.pool().await;
        Ok(images::meta(&pool, slot).await?)
    }

    async fn load_image(&self, id: i64) -> Result<Option<StoredLogo>, DbError> {
        let pool = self.pool().await;
        Ok(images::load(&pool, id).await?)
    }

    async fn load_slot_image(&self, slot: Slot) -> Result<Option<StoredLogo>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(images::load_single(&mut conn, slot).await?)
    }
}
