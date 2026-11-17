use db_core::{
    DbError, MediaRepo,
    media::{Image, Logo, Slot, StoredLogo},
};
use time::OffsetDateTime;

use super::SqliteRepo;
use crate::{competition, images};

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

    async fn set_image_data(&self, slot: Slot, logo: &Logo) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let Some(competition_id) = competition::current_id(&pool).await? else {
            return Ok(false);
        };
        Ok(images::set_data(&pool, &competition_id, slot, logo).await?)
    }

    async fn set_image_url(&self, slot: Slot, url: &str) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        let Some(competition_id) = competition::current_id(&mut *conn).await? else {
            return Ok(false);
        };
        Ok(images::set_url(&mut conn, &competition_id, slot, url).await?)
    }

    async fn remove_image(&self, slot: Slot) -> Result<(), DbError> {
        let pool = self.pool().await;
        Ok(images::remove(&pool, slot).await?)
    }

    async fn add_sponsor(&self, name: &str, link: Option<&str>) -> Result<Option<i64>, DbError> {
        let pool = self.pool().await;
        let Some(competition_id) = competition::current_id(&pool).await? else {
            return Ok(None);
        };
        Ok(Some(
            images::add_sponsor(&pool, &competition_id, name, link).await?,
        ))
    }

    async fn update_sponsor(
        &self,
        id: i64,
        name: &str,
        link: Option<&str>,
    ) -> Result<bool, DbError> {
        let pool = self.pool().await;
        Ok(images::update_sponsor(&pool, id, name, link).await?)
    }

    async fn reorder_sponsors(&self, ids: &[i64]) -> Result<bool, DbError> {
        let mut tx = self.begin_write().await?;
        let mut current: Vec<i64> = images::list(&mut *tx)
            .await?
            .into_iter()
            .filter(|image| image.kind == "sponsor")
            .map(|image| image.id)
            .collect();
        let mut wanted = ids.to_vec();
        current.sort_unstable();
        wanted.sort_unstable();
        if current != wanted {
            return Ok(false);
        }
        for (position, id) in (0..).zip(ids) {
            images::set_position(&mut *tx, *id, position).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn store_fetched_image(
        &self,
        id: i64,
        url: &str,
        result: Result<Logo, String>,
    ) -> Result<bool, DbError> {
        let pool = self.pool().await;
        Ok(images::store_fetched(&pool, id, url, result).await?)
    }
}
