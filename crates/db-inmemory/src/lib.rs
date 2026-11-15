//! A fresh, migrated, in-memory SQLite database (#238): a thin wrapper over
//! `db-sqlite` for tests, dev tooling and scratch databases. Every call gets
//! its own database, which lives as long as its pool.
//!
//! ```ignore
//! let db = db_inmemory::handle().await; // the `DbHandle` the app passes around
//! let repo = db_inmemory::new().await;  // the `SqliteRepo`, for raw fixtures
//! ```

use db_core::DbHandle;
use db_sqlite::{MIGRATOR, SqliteRepo, reloadable_sqlite};
use sqlx::SqlitePool;

pub mod fixtures;

/// A new, empty, unmigrated in-memory pool.
pub async fn unmigrated_pool() -> SqlitePool {
    reloadable_sqlite::in_memory().await
}

/// A new in-memory pool with the migrations applied.
pub async fn pool() -> SqlitePool {
    let pool = unmigrated_pool().await;
    MIGRATOR
        .run(&pool)
        .await
        .expect("migrations apply to an empty database");
    pool
}

/// A repo over a new migrated in-memory database.
pub async fn new() -> SqliteRepo {
    SqliteRepo::from_pool(pool().await)
}

/// [`new`] as the handle the application passes around.
pub async fn handle() -> DbHandle {
    new().await.into_handle()
}
