//! Snapshots of the database file, taken before migrations (and reusable for
//! other backup paths).
//!
//! A backup is a plain SQLite file next to the database, named
//! `<db-stem>.backup-<YYYY-MM-DDTHH-MM-SS>.sqlite` in local time (no colons, so
//! the name works on every file system). It is written with `VACUUM INTO`: a
//! consistent single-file copy, which fits the rollback-journal setup.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use sqlx::{SqlitePool, migrate::Migrator};
use thiserror::Error;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset, macros::format_description};

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("not a file database: {0}")]
    NotAFile(PathBuf),
    #[error("couldn't write the backup {path}: {source}")]
    Write { path: PathBuf, source: sqlx::Error },
    #[error("couldn't read the migration state: {0}")]
    State(#[source] sqlx::Error),
    #[error("no such backup: {0}")]
    NoSuchBackup(String),
    #[error("couldn't list the backups in {dir}: {source}")]
    List {
        dir: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

static LOCAL_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

/// Determines the local UTC offset and remembers it for [`now_local`].
///
/// The `time` crate only reads the offset soundly while the process is
/// single-threaded (on Unix), so binaries call this first thing in a plain
/// `fn main`, before building a multi-threaded runtime. Returns the offset, or
/// an error if it couldn't be determined; then [`now_local`] uses UTC.
pub fn init_local_offset() -> Result<UtcOffset, time::error::IndeterminateOffset> {
    let offset = UtcOffset::current_local_offset()?;
    Ok(*LOCAL_OFFSET.get_or_init(|| offset))
}

/// `now` in the offset found by [`init_local_offset`] (UTC if there is none),
/// without sub-seconds.
fn local_time(now: OffsetDateTime) -> PrimitiveDateTime {
    let now = now.to_offset(LOCAL_OFFSET.get().copied().unwrap_or(UtcOffset::UTC));
    PrimitiveDateTime::new(now.date(), now.time().replace_nanosecond(0).unwrap())
}

/// The current local time without sub-seconds. Falls back to UTC if
/// [`init_local_offset`] wasn't called or failed.
pub fn now_local() -> PrimitiveDateTime {
    local_time(OffsetDateTime::now_utc())
}

/// The part of a database file's name before the extension.
fn stem(db_path: &Path) -> String {
    db_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "db".to_string())
}

/// The backup file name for a database at `db_path` and a local time, in the
/// same directory: `db.sqlite` -> `db.backup-2026-11-14T09-30-00.sqlite`.
pub fn backup_path(db_path: &Path, at: PrimitiveDateTime) -> PathBuf {
    let stamp = at
        .format(format_description!(
            "[year]-[month]-[day]T[hour]-[minute]-[second]"
        ))
        .expect("valid timestamp format");
    db_path.with_file_name(format!("{}.backup-{stamp}.sqlite", stem(db_path)))
}

/// Writes a snapshot of the open database to `target` with `VACUUM INTO`.
/// Fails if `target` already exists or can't be written.
pub async fn create_backup_at(pool: &SqlitePool, target: &Path) -> Result<(), BackupError> {
    sqlx::query("VACUUM INTO ?")
        .bind(target.to_string_lossy().into_owned())
        .execute(pool)
        .await
        .map_err(|source| BackupError::Write {
            path: target.to_path_buf(),
            source,
        })?;
    Ok(())
}

/// Backs up the database at `db_path` next to it, named for the current time.
/// Returns the backup's path.
pub async fn create_backup(pool: &SqlitePool, db_path: &Path) -> Result<PathBuf, BackupError> {
    let target = backup_path(db_path, now_local());
    create_backup_at(pool, &target).await?;
    Ok(target)
}

/// The backups of the database at `db_path`, oldest first (the timestamp in
/// the name sorts chronologically). Empty if the directory has none.
pub fn list_backups(db_path: &Path) -> Result<Vec<PathBuf>, BackupError> {
    let dir = match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let prefix = format!("{}.backup-", stem(db_path));
    let list_err = |source| BackupError::List {
        dir: dir.to_path_buf(),
        source,
    };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(list_err)? {
        let path = entry.map_err(list_err)?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with(&prefix) && name.ends_with(".sqlite") && path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// The local time in a backup's file name, if `name` is exactly
/// `<db-stem>.backup-<YYYY-MM-DDTHH-MM-SS>.sqlite` for the database at
/// `db_path` (what [`backup_path`] produces); nothing else matches.
#[must_use]
pub fn parse_backup_name(db_path: &Path, name: &str) -> Option<PrimitiveDateTime> {
    let stamp = name
        .strip_prefix(&format!("{}.backup-", stem(db_path)))?
        .strip_suffix(".sqlite")?;
    let format = format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let at = PrimitiveDateTime::parse(stamp, format).ok()?;
    // Parsing is lenient (e.g. about padding); only the canonical name counts.
    (backup_path(db_path, at).file_name()?.to_str()? == name).then_some(at)
}

/// The path of the backup called `name` of the database at `db_path`. `name`
/// must match the backup naming exactly **and** be in the directory listing, so
/// the database itself, other files and paths with separators or `..` never
/// resolve.
pub fn resolve_backup(db_path: &Path, name: &str) -> Result<PathBuf, BackupError> {
    let not_found = || BackupError::NoSuchBackup(name.to_string());
    parse_backup_name(db_path, name).ok_or_else(not_found)?;
    list_backups(db_path)?
        .into_iter()
        .find(|path| path.file_name().is_some_and(|n| n == name))
        .ok_or_else(not_found)
}

/// Whether the database has applied migrations and the migrator knows more.
/// A database without migration history (fresh, or empty) has nothing worth
/// backing up, so it counts as not pending.
pub async fn has_pending_migrations(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> Result<bool, BackupError> {
    let table: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await
    .map_err(BackupError::State)?;
    if table.is_none() {
        return Ok(false);
    }
    let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await
        .map_err(BackupError::State)?;
    if applied.is_empty() {
        return Ok(false);
    }
    Ok(migrator
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .any(|m| !applied.contains(&m.version)))
}

/// The versions in `_sqlx_migrations`, sorted.
async fn applied_versions(pool: &SqlitePool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
}

/// Whether the database file, or its journal if there is one, was written
/// after `since`. Anything unreadable counts as modified.
fn modified_since(db_path: &Path, since: std::time::SystemTime) -> bool {
    let mut journal = db_path.as_os_str().to_owned();
    journal.push("-journal");
    [db_path.to_path_buf(), PathBuf::from(journal)]
        .iter()
        .any(|path| match std::fs::metadata(path) {
            Ok(meta) => meta.modified().map_or(true, |at| at > since),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        })
}

/// The newest backup of `db_path` whose applied migrations are exactly the
/// live database's and that the database hasn't been written to since, if
/// there is one. Anything unreadable counts as "no".
async fn newest_backup_with_same_state(pool: &SqlitePool, db_path: &Path) -> Option<PathBuf> {
    let newest = list_backups(db_path).ok()?.pop()?;
    let written = std::fs::metadata(&newest).ok()?.modified().ok()?;
    if modified_since(db_path, written) {
        return None;
    }
    let live = applied_versions(pool).await.ok()?;
    let backup_pool = SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&newest)
            .read_only(true),
    )
    .await
    .ok()?;
    let backed_up = applied_versions(&backup_pool).await;
    backup_pool.close().await;
    (backed_up.ok()? == live).then_some(newest)
}

/// Applies the pending migrations of the embedded `./migrations`, after a
/// backup of the database file at `db_path` if there are any. A failed backup
/// aborts before anything is migrated.
pub async fn migrate_with_backup(pool: &SqlitePool, db_path: &Path) -> Result<(), BackupError> {
    migrate_with_backup_using(&crate::MIGRATOR, pool, db_path, now_local()).await
}

/// [`migrate_with_backup`] with an explicit migrator and time.
pub async fn migrate_with_backup_using(
    migrator: &Migrator,
    pool: &SqlitePool,
    db_path: &Path,
    now: PrimitiveDateTime,
) -> Result<(), BackupError> {
    if has_pending_migrations(pool, migrator).await? {
        if let Some(existing) = newest_backup_with_same_state(pool, db_path).await {
            // A failing migration under a restart loop would otherwise write
            // a full copy on every start.
            tracing::info!(
                "Pending migrations: skipping the backup, {} has the same migration state and the database is unchanged since",
                existing.display()
            );
        } else {
            let target = backup_path(db_path, now);
            tracing::info!(
                "Pending migrations: backing up the database to {}",
                target.display()
            );
            create_backup_at(pool, &target).await?;
        }
    }
    migrator.run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use time::macros::datetime;

    const T: PrimitiveDateTime = datetime!(2026-11-14 09:30:00);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("db-backup-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn db(&self) -> PathBuf {
            self.0.join("db.sqlite")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn open(path: &Path) -> SqlitePool {
        SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap()
    }

    /// The real migrator cut down to its first `n` migrations (up and down).
    fn partial(n: usize) -> Migrator {
        let full = &crate::MIGRATOR;
        let ups: Vec<_> = full
            .iter()
            .filter(|m| m.migration_type.is_up_migration())
            .take(n)
            .cloned()
            .collect();
        let last = ups.last().unwrap().version;
        let mut m = Migrator::DEFAULT;
        m.migrations = full
            .iter()
            .filter(|m| m.version <= last)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        m
    }

    #[test]
    fn local_time_uses_the_stored_offset_and_drops_subseconds() {
        use time::macros::offset;
        // OnceLock: this is the only test that sets it; others never read it.
        let _ = LOCAL_OFFSET.set(offset!(+2));
        let utc = datetime!(2026-11-14 23:30:00.75).assume_utc();
        assert_eq!(local_time(utc), datetime!(2026-11-15 01:30:00));
    }

    #[test]
    fn names_the_backup_next_to_the_db() {
        assert_eq!(
            backup_path(Path::new("/srv/data/db.sqlite"), T),
            PathBuf::from("/srv/data/db.backup-2026-11-14T09-30-00.sqlite")
        );
        assert_eq!(
            backup_path(Path::new("db.sqlite"), T),
            PathBuf::from("db.backup-2026-11-14T09-30-00.sqlite")
        );
    }

    #[tokio::test]
    async fn pending_migrations_get_one_backup_with_the_old_schema() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        partial(1).run(&pool).await.unwrap();

        let full = &crate::MIGRATOR;
        migrate_with_backup_using(full, &pool, &dir.db(), T)
            .await
            .unwrap();

        let backups = list_backups(&dir.db()).unwrap();
        assert_eq!(backups, vec![backup_path(&dir.db(), T)]);
        let backup = open(&backups[0]).await;
        let old: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&backup)
            .await
            .unwrap();
        assert_eq!(old, 1);
        let now: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(now > 1);
    }

    #[tokio::test]
    async fn an_up_to_date_db_gets_no_backup() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        let full = &crate::MIGRATOR;
        full.run(&pool).await.unwrap();
        migrate_with_backup_using(full, &pool, &dir.db(), T)
            .await
            .unwrap();
        assert!(list_backups(&dir.db()).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_fresh_db_gets_no_backup() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        migrate_with_backup_using(&crate::MIGRATOR, &pool, &dir.db(), T)
            .await
            .unwrap();
        assert!(list_backups(&dir.db()).unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unwritable_target_aborts_before_migrating() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        partial(1).run(&pool).await.unwrap();
        // A directory where the backup should go can't be written to.
        std::fs::create_dir(backup_path(&dir.db(), T)).unwrap();

        let result = migrate_with_backup_using(&crate::MIGRATOR, &pool, &dir.db(), T).await;

        assert!(matches!(result, Err(BackupError::Write { .. })));
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(applied, 1, "nothing was migrated");
    }

    #[tokio::test]
    async fn lists_only_this_dbs_backups_oldest_first() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        let later = datetime!(2026-11-15 08:00:00);
        create_backup_at(&pool, &backup_path(&dir.db(), later))
            .await
            .unwrap();
        create_backup_at(&pool, &backup_path(&dir.db(), T))
            .await
            .unwrap();
        std::fs::write(dir.0.join("other.backup-2026-01-01T00-00-00.sqlite"), "").unwrap();
        std::fs::write(dir.0.join("db.sqlite-journal"), "").unwrap();
        assert_eq!(
            list_backups(&dir.db()).unwrap(),
            vec![backup_path(&dir.db(), T), backup_path(&dir.db(), later)]
        );
    }

    #[test]
    fn only_the_exact_backup_name_parses() {
        let db = Path::new("/srv/db.sqlite");
        assert_eq!(
            parse_backup_name(db, "db.backup-2026-11-14T09-30-00.sqlite"),
            Some(T)
        );
        for bad in [
            "db.sqlite",
            "../db.backup-2026-11-14T09-30-00.sqlite",
            "/srv/db.backup-2026-11-14T09-30-00.sqlite",
            "other.backup-2026-11-14T09-30-00.sqlite",
            "db.backup-2026-11-14T09-30-00.sqlite.bak",
            "db.backup-2026-11-14T9-30-00.sqlite",
            "db.backup-2026-11-14T09-30-00 .sqlite",
        ] {
            assert_eq!(parse_backup_name(db, bad), None, "{bad}");
        }
    }

    /// The first `n` real migrations plus one that fails.
    fn failing_after(n: usize) -> Migrator {
        let mut migrations = partial(n).migrations.to_vec();
        migrations.push(sqlx::migrate::Migration::new(
            99_991_231_000_000,
            "broken".into(),
            sqlx::migrate::MigrationType::Simple,
            sqlx::SqlStr::from_static("THIS IS NOT SQL"),
            false,
        ));
        let mut m = Migrator::DEFAULT;
        m.migrations = migrations.into();
        m
    }

    #[tokio::test]
    async fn a_crash_looping_migration_backs_up_only_once() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        partial(1).run(&pool).await.unwrap();
        let broken = failing_after(1);

        for second in 0..3 {
            let at = T + time::Duration::seconds(second);
            assert!(
                migrate_with_backup_using(&broken, &pool, &dir.db(), at)
                    .await
                    .is_err()
            );
        }

        assert_eq!(
            list_backups(&dir.db()).unwrap(),
            vec![backup_path(&dir.db(), T)]
        );
    }

    #[tokio::test]
    async fn a_changed_migration_state_gets_a_new_backup() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        partial(1).run(&pool).await.unwrap();
        assert!(
            migrate_with_backup_using(&failing_after(1), &pool, &dir.db(), T)
                .await
                .is_err()
        );

        // The state moves on (an update applied one more migration), then
        // the next pending migration fails.
        partial(2).run(&pool).await.unwrap();
        let later = T + time::Duration::seconds(5);
        assert!(
            migrate_with_backup_using(&failing_after(2), &pool, &dir.db(), later)
                .await
                .is_err()
        );

        assert_eq!(
            list_backups(&dir.db()).unwrap(),
            vec![backup_path(&dir.db(), T), backup_path(&dir.db(), later)]
        );
    }

    #[tokio::test]
    async fn a_write_since_the_newest_backup_gets_a_new_backup() {
        let dir = TempDir::new();
        let pool = open(&dir.db()).await;
        partial(1).run(&pool).await.unwrap();

        // A manual backup at the current migration state, long ago.
        let manual = backup_path(&dir.db(), T);
        create_backup_at(&pool, &manual).await.unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&manual)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();

        // Weeks of data follow, then a release with a new migration.
        sqlx::query("CREATE TABLE weeks_of_data (x)")
            .execute(&pool)
            .await
            .unwrap();
        let later = T + time::Duration::seconds(5);
        migrate_with_backup_using(&crate::MIGRATOR, &pool, &dir.db(), later)
            .await
            .unwrap();

        assert_eq!(
            list_backups(&dir.db()).unwrap(),
            vec![manual, backup_path(&dir.db(), later)]
        );
    }
}
