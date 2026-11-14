//! Restoring the database from a backup (#354).
//!
//! [`check_backup`] looks at a backup read-only. [`restore`] swaps it in under
//! the pool's write lock: pre-restore snapshot, close, atomic rename, reopen,
//! migrate; any failure after the close puts the snapshot back.

use crate::ids::ActId;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;
use time::PrimitiveDateTime;
use utoipa::ToSchema;

use crate::{
    db_backup::{self, BackupError},
    reloadable_sqlite::{self, ReloadableSqlite},
};

/// What a look at a backup found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BackupCheck {
    /// Reasons the backup can never be restored (German).
    pub blocking: Vec<String>,
    /// Migrations the backup lacks (older schema), by name.
    pub pending_migrations: Vec<String>,
}

/// An act whose song file isn't in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct MissingSong {
    pub act_id: ActId,
    /// The act's name, or the group's.
    pub name: String,
    /// The act's club(s), if known.
    pub club: Option<String>,
    /// The file name of the original upload.
    pub song_file_name: Option<String>,
    /// The expected file, relative to the data directory.
    pub song_file: String,
}

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("the backup can't be restored: {0:?}")]
    Blocking(Vec<String>),
    #[error("the backup needs migrations: {0:?}")]
    PendingMigrations(Vec<String>),
    #[error("not a file database")]
    NotAFile,
    #[error(transparent)]
    Backup(#[from] BackupError),
    /// Failed before anything changed.
    #[error("{0}")]
    Before(String),
    /// Failed after the swap began; the snapshot is back in place.
    #[error("restoring failed, the previous state is back: {0}")]
    RolledBack(String),
    /// Failed and the snapshot couldn't be put back either.
    #[error("restoring failed and the rollback failed too: {cause}; rollback: {rollback}")]
    RollbackFailed { cause: String, rollback: String },
}

async fn open_read_only(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
}

/// Opens the backup read-only and runs the checks of RST-3. The file itself
/// is never modified.
pub async fn check_backup(path: &Path, migrator: &Migrator) -> BackupCheck {
    let invalid = || BackupCheck {
        blocking: vec!["Die Datei ist keine gültige SQLite-Datenbank.".into()],
        pending_migrations: vec![],
    };
    let Ok(pool) = open_read_only(path).await else {
        return invalid();
    };
    let result = check_pool(&pool, migrator).await;
    pool.close().await;
    result.unwrap_or_else(|_| invalid())
}

async fn check_pool(pool: &SqlitePool, migrator: &Migrator) -> Result<BackupCheck, sqlx::Error> {
    let mut check = BackupCheck::default();
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(pool)
        .await?;
    if integrity != ["ok"] {
        check
            .blocking
            .push("Die Integritätsprüfung der Datenbank ist fehlgeschlagen.".into());
        return Ok(check);
    }

    let has_table = |name: &'static str| async move {
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map(|t| t.is_some())
    };

    // Admin: `user_roles` since the roles migration, `users.is_admin` before.
    let admins: i64 = if has_table("user_roles").await? {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_roles JOIN users ON users.id = user_roles.user_id \
             WHERE user_roles.role = 'admin'",
        )
        .fetch_one(pool)
        .await?
    } else if has_table("users").await? {
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin")
            .fetch_one(pool)
            .await?
    } else {
        0
    };
    if admins == 0 {
        check
            .blocking
            .push("Im Backup gibt es keinen Benutzer mit der Rolle Admin.".into());
    }

    if !has_table("_sqlx_migrations").await? {
        check
            .blocking
            .push("Das Backup enthält keinen Migrationsstand.".into());
        return Ok(check);
    }
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;
    let ups: Vec<_> = migrator
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .collect();
    let mut newer = false;
    for (version, checksum) in &applied {
        match ups.iter().find(|m| m.version == *version) {
            None => newer = true,
            Some(m) if m.checksum.as_ref() != checksum.as_slice() => newer = true,
            Some(_) => {}
        }
    }
    if newer {
        check.blocking.push(
            "Das Backup stammt aus einer neueren Version (unbekannte oder geänderte \
             Migrationen) und kann mit dieser Version nicht wiederhergestellt werden."
                .into(),
        );
    }
    check.pending_migrations = ups
        .iter()
        .filter(|m| !applied.iter().any(|(v, _)| *v == m.version))
        .map(|m| m.description.to_string())
        .collect();
    Ok(check)
}

/// Acts whose original song file doesn't exist in `data_path`.
///
/// Tries the club names of the current schema; a database with an older
/// schema (a backup before its migrations) gets the names without clubs.
pub async fn missing_songs(
    pool: &SqlitePool,
    data_path: &Path,
) -> Result<Vec<MissingSong>, sqlx::Error> {
    type Row = (ActId, String, Option<String>, Option<String>, String);
    let rich = sqlx::query_as::<_, Row>(
        r#"
        SELECT acts.id, COALESCE(acts.group_name, acts.name),
            COALESCE(
                (SELECT clubs.name FROM clubs WHERE clubs.id = acts.owner_club_id),
                (SELECT group_concat(DISTINCT clubs.name)
                 FROM act_participants ap
                 JOIN rider ON rider.id = ap.rider_id
                 JOIN clubs ON clubs.id = rider.club_id
                 WHERE ap.act_id = acts.id)
            ),
            acts.song_file_name, acts.song_file
        FROM acts WHERE acts.song_file IS NOT NULL
        ORDER BY COALESCE(acts.group_name, acts.name)
        "#,
    )
    .fetch_all(pool)
    .await;
    let rows = match rich {
        Ok(rows) => rows,
        Err(_) => sqlx::query_as::<_, (ActId, String, Option<String>, String)>(
            "SELECT id, COALESCE(group_name, name), song_file_name, song_file \
             FROM acts WHERE song_file IS NOT NULL ORDER BY COALESCE(group_name, name)",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(id, name, file_name, file)| (id, name, None, file_name, file))
        .collect(),
    };
    Ok(rows
        .into_iter()
        .filter(|(.., song_file)| !data_path.join(song_file).is_file())
        .map(
            |(act_id, name, club, song_file_name, song_file)| MissingSong {
                act_id,
                name,
                club,
                song_file_name,
                song_file,
            },
        )
        .collect())
}

/// [`missing_songs`] for the backup file, opened read-only.
pub async fn missing_songs_in_backup(
    path: &Path,
    data_path: &Path,
) -> Result<Vec<MissingSong>, sqlx::Error> {
    let pool = open_read_only(path).await?;
    let result = missing_songs(&pool, data_path).await;
    pool.close().await;
    result
}

/// The outcome of a successful [`restore`].
#[derive(Debug, Clone)]
pub struct Restored {
    /// The pre-restore snapshot; restoring it undoes the restore.
    pub snapshot: PathBuf,
}

/// Restores `backup` with the embedded migrations. See [`restore_using`].
pub async fn restore(
    db: &ReloadableSqlite,
    backup: &Path,
    force: bool,
) -> Result<Restored, RestoreError> {
    restore_using(
        db,
        backup,
        force,
        &sqlx::migrate!("./migrations"),
        db_backup::now_local(),
    )
    .await
}

/// Replaces the database with `backup` (RST-1…RST-4).
///
/// Everything happens under the pool's write lock. Blocking findings always
/// refuse, pending migrations refuse unless `force`. The backup file is never
/// modified; the restored database is migrated without another backup.
pub async fn restore_using(
    db: &ReloadableSqlite,
    backup: &Path,
    force: bool,
    migrator: &Migrator,
    now: PrimitiveDateTime,
) -> Result<Restored, RestoreError> {
    let db_path = db.file_path().ok_or(RestoreError::NotAFile)?;
    let mut pool = db.write().await;

    let check = check_backup(backup, migrator).await;
    if !check.blocking.is_empty() {
        return Err(RestoreError::Blocking(check.blocking));
    }
    if !check.pending_migrations.is_empty() && !force {
        return Err(RestoreError::PendingMigrations(check.pending_migrations));
    }

    // A backup of the same second may exist already (e.g. made by hand just
    // before): take the next free second.
    let mut at = now;
    let snapshot = loop {
        let candidate = db_backup::backup_path(&db_path, at);
        if !candidate.exists() {
            break candidate;
        }
        at += time::Duration::seconds(1);
    };
    db_backup::create_backup_at(&pool, &snapshot).await?;

    pool.close().await;
    let swapped = swap_in(db, &db_path, backup, migrator).await;
    match swapped {
        Ok(new_pool) => {
            *pool = new_pool;
            Ok(Restored { snapshot })
        }
        Err(cause) => match swap_in_snapshot(db, &db_path, &snapshot).await {
            Ok(old_pool) => {
                *pool = old_pool;
                Err(RestoreError::RolledBack(cause))
            }
            Err(rollback) => Err(RestoreError::RollbackFailed { cause, rollback }),
        },
    }
}

/// Copies `source` next to the database and renames it over it, atomically.
fn replace_file(db_path: &Path, source: &Path) -> Result<(), String> {
    let name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = db_path.with_file_name(format!("{name}.restore-tmp"));
    std::fs::copy(source, &temp).map_err(|e| format!("copy: {e}"))?;
    std::fs::rename(&temp, db_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("rename: {e}")
    })
}

/// Steps 4 to 6: replace the file, reopen, migrate. On failure the new pool
/// is closed again.
async fn swap_in(
    db: &ReloadableSqlite,
    db_path: &Path,
    backup: &Path,
    migrator: &Migrator,
) -> Result<SqlitePool, String> {
    replace_file(db_path, backup)?;
    let pool = reloadable_sqlite::connect(db.url())
        .await
        .map_err(|e| format!("reconnect: {e}"))?;
    if let Err(e) = migrator.run(&pool).await {
        pool.close().await;
        return Err(format!("migration: {e}"));
    }
    Ok(pool)
}

/// The rollback: the snapshot over the database, reopened.
async fn swap_in_snapshot(
    db: &ReloadableSqlite,
    db_path: &Path,
    snapshot: &Path,
) -> Result<SqlitePool, String> {
    replace_file(db_path, snapshot)?;
    reloadable_sqlite::connect(db.url())
        .await
        .map_err(|e| format!("reconnect: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::UserId;
    use sqlx::migrate::{Migration, MigrationType};
    use time::macros::datetime;

    const T: PrimitiveDateTime = datetime!(2026-11-14 09:30:00);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("db-restore-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn names(db: &ReloadableSqlite) -> Vec<String> {
        sqlx::query_scalar("SELECT name FROM users ORDER BY name")
            .fetch_all(&*db.get().await)
            .await
            .unwrap()
    }

    async fn add_admin(pool: &SqlitePool, name: &str) {
        let id = UserId::new();
        sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES (?, ?, true, ?, 'x')")
            .bind(id)
            .bind(format!("{id}@example.org"))
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO user_roles (user_id, role) VALUES (?, 'admin')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    /// The embedded migrations plus one that fails.
    fn failing_migrator() -> Migrator {
        let full = sqlx::migrate!("./migrations");
        let mut migrations = full.migrations.to_vec();
        migrations.push(Migration::new(
            99_991_231_000_000,
            "broken".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static("THIS IS NOT SQL"),
            false,
        ));
        let mut migrator = Migrator::DEFAULT;
        migrator.migrations = migrations.into();
        migrator
    }

    #[tokio::test]
    async fn a_failing_migration_puts_the_pre_restore_snapshot_back() {
        let dir = TempDir::new();
        let path = dir.0.join("db.sqlite");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = reloadable_sqlite::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        add_admin(&pool, "Vorher").await;
        let db = ReloadableSqlite::new(pool.clone(), url);
        let backup = db_backup::backup_path(&path, datetime!(2026-01-01 00:00:00));
        db_backup::create_backup_at(&pool, &backup).await.unwrap();
        add_admin(&pool, "Nach dem Backup").await;
        let backup_bytes = std::fs::read(&backup).unwrap();

        let result = restore_using(&db, &backup, true, &failing_migrator(), T).await;

        assert!(
            matches!(result, Err(RestoreError::RolledBack(_))),
            "{result:?}"
        );
        // The live data are back, and the server has a working database.
        assert_eq!(names(&db).await, ["Nach dem Backup", "Vorher"]);
        add_admin(&*db.get().await, "Danach").await;
        assert!(db_backup::backup_path(&path, T).exists());
        assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
        assert!(!path.with_file_name("db.sqlite.restore-tmp").exists());
    }
}
