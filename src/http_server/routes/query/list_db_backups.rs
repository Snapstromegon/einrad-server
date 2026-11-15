use axum::{Extension, Json};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    db_backup,
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::db_file},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DbBackup {
    /// The file name, e.g. `db.backup-2026-11-14T09-30-00.sqlite`.
    pub filename: String,
    /// When the backup was made, in server local time without offset, taken
    /// from the name: `2026-11-14T09:30:00`.
    pub created_at: String,
    pub size_bytes: u64,
}

/// List the database backups next to the database file, newest first.
///
/// Admins only: backups contain password hashes, emails and payment data.
#[utoipa::path(
    get,
    tags=["query", "admin"],
    path="/list_db_backups",
    responses(
        (status=200, content_type="application/json", body=Vec<DbBackup>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_db_backups(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageBackups>,
) -> Result<Json<Vec<DbBackup>>, HttpError> {
    let db_path = db_file(&db)?;
    let mut backups = Vec::new();
    for path in db_backup::list_backups(&db_path)? {
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(created) = db_backup::parse_backup_name(&db_path, filename) else {
            continue;
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        backups.push(DbBackup {
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
    Ok(Json(backups))
}
