//! Runs the backend conformance suite against a fresh in-memory SQLite.

use db_sqlite::SqliteRepo;

db_core::conformance_tests!(|| async { SqliteRepo::in_memory().await.into_handle() });
