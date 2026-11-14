//! [`SqliteRepo`]: the SQLite implementation of [`db_core::Db`].
//!
//! All SQL of the application lives in the `repo/` modules, one per domain,
//! checked at compile time by the `sqlx::query!` macros against the SQLite
//! schema (`DATABASE_URL`, see the makefile).

use std::sync::Arc;

use db_core::{Db, DbError, DbHandle};
use sqlx::SqlitePool;

use crate::reloadable_sqlite::ReloadableSqlite;

mod auth;
mod clubs;
mod groups;
mod riders;
mod settings;
mod users;

/// The SQLite backend. A cheap clone: it shares the [`ReloadableSqlite`], so
/// a reload or a restore is seen by every clone.
#[derive(Debug, Clone)]
pub struct SqliteRepo {
    db: ReloadableSqlite,
}

impl SqliteRepo {
    #[must_use]
    pub fn new(db: ReloadableSqlite) -> Self {
        Self { db }
    }

    /// A repo over a pool that nobody reloads, e.g. an in-memory database in
    /// a test.
    #[must_use]
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self::new(ReloadableSqlite::new(pool, "sqlite::memory:".to_string()))
    }

    /// The repo as the handle the application passes around.
    #[must_use]
    pub fn into_handle(self) -> DbHandle {
        Arc::new(self)
    }

    /// The current pool. Clone it out of the guard so a reload isn't held up.
    async fn pool(&self) -> SqlitePool {
        self.db.get().await.clone()
    }

    /// Begins a write transaction (`BEGIN IMMEDIATE`).
    async fn begin_write(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, DbError> {
        Ok(crate::reloadable_sqlite::begin_write(&self.pool().await).await?)
    }
}

#[async_trait::async_trait]
impl Db for SqliteRepo {
    async fn reload(&self) -> Result<(), DbError> {
        Ok(self.db.reload().await?)
    }
}
