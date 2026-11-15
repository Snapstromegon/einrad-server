//! [`SqliteRepo`]: the SQLite implementation of [`db_core::Db`].
//!
//! All SQL of the application lives in the `repo/` modules, one per domain,
//! checked at compile time by the `sqlx::query!` macros against the SQLite
//! schema (`DATABASE_URL`, see the makefile).

use std::sync::Arc;

use db_core::{Db, DbError, DbHandle};
use sqlx::SqlitePool;

use crate::reloadable_sqlite::ReloadableSqlite;

mod acts;
mod admin_commands;
mod auth;
mod backups;
mod bookings;
mod categories;
mod check_in;
mod club_commands;
mod club_mails;
mod club_queries;
mod clubs;
mod competition;
mod export;
mod fees;
mod group_commands;
mod group_queries;
mod groups;
mod heats_finals;
mod jury;
mod media;
mod metrics;
mod riders;
mod seed;
mod settings;
mod timeplan;
mod users;

/// The SQLite backend. A cheap clone: it shares the [`ReloadableSqlite`], so
/// a reload or a restore is seen by every clone.
#[derive(Debug, Clone)]
pub struct SqliteRepo {
    db: ReloadableSqlite,
    /// Whether [`Db::backups`] offers the file backups; only a test turns it
    /// off (see [`SqliteRepo::without_backups`]), to stand in for a backend without them.
    backups: bool,
}

impl SqliteRepo {
    #[must_use]
    pub fn new(db: ReloadableSqlite) -> Self {
        Self { db, backups: true }
    }

    /// A repo over a pool that nobody reloads, e.g. an in-memory database in
    /// a test.
    #[must_use]
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self::new(ReloadableSqlite::new(pool, "sqlite::memory:".to_string()))
    }

    /// The same repo, but [`Db::backups`] answers `None`, like a backend
    /// without file backups.
    #[must_use]
    pub fn without_backups(mut self) -> Self {
        self.backups = false;
        self
    }

    /// The repo as the handle the application passes around.
    #[must_use]
    pub fn into_handle(self) -> DbHandle {
        Arc::new(self)
    }

    /// The current pool. Clone it out of the guard so a reload isn't held up.
    pub async fn pool(&self) -> SqlitePool {
        self.db.get().await.clone()
    }

    /// Begins a write transaction (`BEGIN IMMEDIATE`).
    async fn begin_write(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, DbError> {
        Ok(crate::reloadable_sqlite::begin_write(&self.pool().await).await?)
    }

    /// Begins a read transaction: every statement in it sees one snapshot.
    async fn begin_read(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, DbError> {
        Ok(self.pool().await.begin().await?)
    }
}

/// The "begin write -> body -> commit" shape of a command:
/// `write_tx!(self, tx, { ... })` evaluates the block with `tx` bound to a
/// `BEGIN IMMEDIATE` transaction (`&mut *tx` is the connection), commits and
/// yields the block's value. An early `?` in the block drops `tx`, which rolls
/// back. Begin and commit errors are returned with `?`, so the enclosing
/// function's error type needs `From<sqlx::Error>` or `From<DbError>`.
macro_rules! write_tx {
    ($repo:expr, $tx:ident, $body:block) => {{
        let mut $tx = $repo.begin_write().await?;
        let value = $body;
        $tx.commit().await?;
        value
    }};
}

/// The read counterpart of [`write_tx!`]: one read transaction, so a query
/// that combines several reads sees one snapshot. Nothing to commit; the
/// transaction ends with the block.
macro_rules! read_tx {
    ($repo:expr, $tx:ident, $body:block) => {{
        let mut $tx = $repo.begin_read().await?;
        $body
    }};
}
pub(crate) use {read_tx, write_tx};

// The CQRS area traits (#238): empty until the endpoints migrate, one impl
// per area, in the area's own file from then on.

impl db_core::commands::CategoryCommands for SqliteRepo {}
impl db_core::queries::CategoryQueries for SqliteRepo {}
impl db_core::commands::HeatCommands for SqliteRepo {}
impl db_core::queries::HeatQueries for SqliteRepo {}
impl db_core::commands::FinalCommands for SqliteRepo {}
impl db_core::queries::FinalQueries for SqliteRepo {}
impl db_core::commands::TimeplanCommands for SqliteRepo {}
impl db_core::queries::TimeplanQueries for SqliteRepo {}
impl db_core::commands::JuryCommands for SqliteRepo {}
impl db_core::queries::JuryQueries for SqliteRepo {}
impl db_core::commands::CompetitionCommands for SqliteRepo {}
impl db_core::queries::CompetitionQueries for SqliteRepo {}
impl db_core::commands::MediaCommands for SqliteRepo {}
impl db_core::queries::MediaQueries for SqliteRepo {}
impl db_core::commands::UserCommands for SqliteRepo {}
impl db_core::queries::UserQueries for SqliteRepo {}
impl db_core::queries::AdminQueries for SqliteRepo {}
impl db_core::commands::ActCommands for SqliteRepo {}
impl db_core::queries::ActQueries for SqliteRepo {}
impl db_core::support::Support for SqliteRepo {}

#[async_trait::async_trait]
impl Db for SqliteRepo {
    async fn reload(&self) -> Result<(), DbError> {
        Ok(self.db.reload().await?)
    }

    fn backups(&self) -> Option<&dyn db_core::backups::Backups> {
        self.backups
            .then_some(self as &dyn db_core::backups::Backups)
    }
}

#[cfg(test)]
mod tx_tests {
    use super::*;

    async fn repo() -> SqliteRepo {
        let pool = crate::reloadable_sqlite::in_memory().await;
        sqlx::query("CREATE TABLE t (x INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        SqliteRepo::from_pool(pool)
    }

    async fn insert(repo: &SqliteRepo, fail: bool) -> Result<(), DbError> {
        write_tx!(repo, tx, {
            sqlx::query("INSERT INTO t VALUES (1)")
                .execute(&mut *tx)
                .await?;
            if fail {
                sqlx::query("NOT SQL").execute(&mut *tx).await?;
            }
        });
        Ok(())
    }

    async fn count(repo: &SqliteRepo) -> Result<i64, DbError> {
        Ok(read_tx!(repo, tx, {
            sqlx::query_scalar("SELECT COUNT(*) FROM t")
                .fetch_one(&mut *tx)
                .await?
        }))
    }

    #[tokio::test]
    async fn write_tx_commits_and_rolls_back_on_an_early_error() {
        let repo = repo().await;
        insert(&repo, false).await.unwrap();
        assert!(insert(&repo, true).await.is_err());
        assert_eq!(count(&repo).await.unwrap(), 1);
    }
}
