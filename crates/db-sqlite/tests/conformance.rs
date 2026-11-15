//! Runs the backend conformance suite against a fresh in-memory SQLite.
//! (`db-inmemory` runs it too; it depends on this crate, so this one wires
//! the pool by hand instead of depending back on it.)

use db_sqlite::{MIGRATOR, SqliteRepo, reloadable_sqlite};

db_core::conformance_tests!(|| async {
    let pool = reloadable_sqlite::in_memory().await;
    MIGRATOR.run(&pool).await.unwrap();
    SqliteRepo::from_pool(pool).into_handle()
});
