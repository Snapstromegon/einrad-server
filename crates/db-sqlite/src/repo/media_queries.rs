//! `MediaQueries` for SQLite.

use db_core::{
    actor::Actor,
    error::QueryError,
    media::Image,
    queries::media::{MediaQueries, Sponsor},
};

use super::{SqliteRepo, read_tx};
use crate::images;

#[async_trait::async_trait]
impl MediaQueries for SqliteRepo {
    async fn get_competition_images(&self, _actor: &Actor) -> Result<Vec<Image>, QueryError> {
        read_tx!(self, tx, { Ok(images::list(&mut *tx).await?) })
    }

    async fn list_sponsors(&self) -> Result<Vec<Sponsor>, QueryError> {
        read_tx!(self, tx, {
            Ok(Sponsor::listed(images::list(&mut *tx).await?))
        })
    }
}
