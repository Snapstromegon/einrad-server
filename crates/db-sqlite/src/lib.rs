//! The SQLite backend: the reloadable pool, backups and restores, and the
//! migrations (`migrations/`, embedded as [`MIGRATOR`]).

use sqlx::migrate::Migrator;

pub mod db_backup;
pub mod db_restore;
pub mod reloadable_sqlite;
mod repo;

pub use repo::SqliteRepo;

/// All migrations of the SQLite schema.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
