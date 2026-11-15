//! `CompetitionQueries` for SQLite.

use db_core::{
    actor::Actor,
    competition::Competition,
    error::QueryError,
    queries::competition::{CompetitionQueries, SystemStatus},
};

use super::{SqliteRepo, read_tx};
use crate::competition;

#[async_trait::async_trait]
impl CompetitionQueries for SqliteRepo {
    async fn get_competition_settings(&self, _actor: &Actor) -> Result<Competition, QueryError> {
        read_tx!(self, tx, {
            competition::current(&mut *tx)
                .await?
                .ok_or(QueryError::NotFound)
        })
    }

    async fn get_system_status(&self) -> Result<SystemStatus, QueryError> {
        // One snapshot: the competition and its published flag match.
        read_tx!(self, tx, {
            let competition = competition::current(&mut *tx).await?;
            let startlist_published = super::settings::startlist_published(&mut tx).await?;
            Ok(SystemStatus {
                competition,
                startlist_published,
            })
        })
    }
}
