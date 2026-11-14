//! File backups of the database and restoring them: a capability of the
//! backends that keep their data in a file (SQLite), not part of every
//! backend (ADR 0019). Reach it through [`crate::Db::backups`].

use std::path::{Path, PathBuf};

use crate::acts::MissingSong;

/// A backup file next to the database file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub filename: String,
    /// When the backup was made, in server local time without offset, taken
    /// from the name: `2026-11-14T09:30:00`.
    pub created_at: String,
    pub size_bytes: u64,
}

/// What a look at a backup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupCheck {
    /// Why the backup can never be restored (German). Empty if it can.
    pub blocking: Vec<String>,
    /// The migrations the backup lacks (older schema), by name.
    pub pending_migrations: Vec<String>,
    /// Acts whose song file isn't in the data directory.
    pub missing_songs: Vec<MissingSong>,
}

/// Why a backup operation failed.
#[derive(Debug, thiserror::Error)]
pub enum BackupsError {
    /// The database doesn't live in a file, e.g. in memory.
    #[error("the database is not a file")]
    NotAFile,
    #[error("no such backup: {0}")]
    NoSuchBackup(String),
    /// A backup of the same second exists.
    #[error("a backup of this second exists")]
    AlreadyExists,
    /// The backup can never be restored (German reasons).
    #[error("the backup can't be restored: {0:?}")]
    Blocking(Vec<String>),
    /// The backup has an older schema and needs `force` (migration names).
    #[error("the backup needs migrations: {0:?}")]
    PendingMigrations(Vec<String>),
    /// Music is playing.
    #[error("music is playing")]
    Playing,
    /// The restore failed after it began; the previous state is back.
    #[error("restoring failed, the previous state is back: {0}")]
    RolledBack(String),
    #[error("{0}")]
    Failed(String),
}

/// What a restore did.
#[derive(Debug, Clone)]
pub struct Restored {
    /// The file name of the backup of the state before the restore.
    pub snapshot: String,
}

/// Backups and restores of a file database.
#[async_trait::async_trait]
pub trait Backups: Send + Sync {
    /// The path of the existing backup `filename` (for the download).
    fn backup_path(&self, filename: &str) -> Result<PathBuf, BackupsError>;

    /// The backups, newest first.
    fn list(&self) -> Result<Vec<BackupInfo>, BackupsError>;

    /// Backs the database up now; returns the new file's name.
    async fn create(&self) -> Result<String, BackupsError>;

    /// Deletes backups. Every name must be an existing backup; if one isn't,
    /// nothing is deleted.
    fn delete(&self, filenames: &[String]) -> Result<(), BackupsError>;

    /// Looks at a backup read-only. `data_path` is where song files live.
    async fn check(&self, filename: &str, data_path: &Path) -> Result<BackupCheck, BackupsError>;

    /// Replaces the database with a backup. `is_playing` is asked under the
    /// write lock; `true` refuses with [`BackupsError::Playing`].
    async fn restore(
        &self,
        filename: &str,
        force: bool,
        is_playing: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Restored, BackupsError>;
}
