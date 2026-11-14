//! The SQLite backend: the reloadable pool, backups and restores, and the
//! migrations (`migrations/`, embedded as [`MIGRATOR`]).

use sqlx::migrate::Migrator;

pub mod competition;
pub mod db_backup;
pub mod db_restore;
mod fees;
mod group_rules;
pub mod images;
mod registration;
pub mod reloadable_sqlite;
mod repo;
pub mod structure;
mod timeplan;

pub use repo::SqliteRepo;

/// All migrations of the SQLite schema.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
