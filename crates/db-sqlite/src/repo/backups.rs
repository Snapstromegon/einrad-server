//! Backups and restores of the database file: the SQLite backend's
//! [`Backups`] capability.

use std::path::{Path, PathBuf};

use db_core::backups::{BackupCheck, BackupInfo, Backups, BackupsError, Restored};

use super::SqliteRepo;
use crate::{
    db_backup::{self, BackupError},
    db_restore::{self, RestoreError},
};

fn backup_error(e: BackupError) -> BackupsError {
    match e {
        BackupError::NotAFile(_) => BackupsError::NotAFile,
        BackupError::NoSuchBackup(name) => BackupsError::NoSuchBackup(name),
        e => BackupsError::Failed(e.to_string()),
    }
}

fn restore_error(e: RestoreError) -> BackupsError {
    match e {
        RestoreError::NotAFile => BackupsError::NotAFile,
        RestoreError::Backup(e) => backup_error(e),
        RestoreError::Blocking(reasons) => BackupsError::Blocking(reasons),
        RestoreError::PendingMigrations(names) => BackupsError::PendingMigrations(names),
        RestoreError::Playing => BackupsError::Playing,
        RestoreError::RolledBack(cause) => BackupsError::RolledBack(cause),
        e => BackupsError::Failed(e.to_string()),
    }
}

impl SqliteRepo {
    /// The live database file; backups sit next to it.
    fn db_file(&self) -> Result<PathBuf, BackupsError> {
        self.db.file_path().ok_or(BackupsError::NotAFile)
    }
}

#[async_trait::async_trait]
impl Backups for SqliteRepo {
    fn backup_path(&self, filename: &str) -> Result<PathBuf, BackupsError> {
        db_backup::resolve_backup(&self.db_file()?, filename).map_err(backup_error)
    }

    fn list(&self) -> Result<Vec<BackupInfo>, BackupsError> {
        let db_path = self.db_file()?;
        let mut backups = Vec::new();
        for path in db_backup::list_backups(&db_path).map_err(backup_error)? {
            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(created) = db_backup::parse_backup_name(&db_path, filename) else {
                continue;
            };
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            backups.push(BackupInfo {
                filename: filename.to_string(),
                created_at: created
                    .format(time::macros::format_description!(
                        "[year]-[month]-[day]T[hour]:[minute]:[second]"
                    ))
                    .expect("valid timestamp"),
                size_bytes: metadata.len(),
            });
        }
        backups.reverse();
        Ok(backups)
    }

    async fn create(&self) -> Result<String, BackupsError> {
        let target = db_backup::backup_path(&self.db_file()?, db_backup::now_local());
        if target.exists() {
            return Err(BackupsError::AlreadyExists);
        }
        let pool = self.pool().await;
        db_backup::create_backup_at(&pool, &target)
            .await
            .map_err(backup_error)?;
        Ok(target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default())
    }

    fn delete(&self, filenames: &[String]) -> Result<(), BackupsError> {
        let db_path = self.db_file()?;
        // Check all names first, so a bad one deletes nothing.
        let paths = filenames
            .iter()
            .map(|name| db_backup::resolve_backup(&db_path, name))
            .collect::<Result<Vec<_>, _>>()
            .map_err(backup_error)?;
        for path in paths {
            match std::fs::remove_file(&path) {
                Ok(()) => tracing::info!("Deleted database backup {}", path.display()),
                // Deleted meanwhile, which is what was asked for.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(BackupsError::Failed(format!(
                        "couldn't delete {}: {e}",
                        path.display()
                    )));
                }
            }
        }
        Ok(())
    }

    async fn check(&self, filename: &str, data_path: &Path) -> Result<BackupCheck, BackupsError> {
        let path = self.backup_path(filename)?;
        let check = db_restore::check_backup(&path, &crate::MIGRATOR).await;
        // Songs can only be listed from a readable database.
        let missing_songs = if check.blocking.is_empty() {
            db_restore::missing_songs_in_backup(&path, data_path)
                .await
                .map_err(|e| BackupsError::Failed(e.to_string()))?
        } else {
            vec![]
        };
        Ok(BackupCheck {
            blocking: check.blocking,
            pending_migrations: check.pending_migrations,
            missing_songs,
        })
    }

    async fn restore(
        &self,
        filename: &str,
        force: bool,
        is_playing: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Restored, BackupsError> {
        let path = self.backup_path(filename)?;
        let restored = db_restore::restore(&self.db, &path, force, is_playing)
            .await
            .map_err(restore_error)?;
        Ok(Restored {
            snapshot: restored
                .snapshot
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        })
    }
}
