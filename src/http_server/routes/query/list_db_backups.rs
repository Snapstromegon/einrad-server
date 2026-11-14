use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::backups},
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
        (status=501, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_db_backups(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageBackups>,
) -> Result<Json<Vec<DbBackup>>, HttpError> {
    let backups = backups(&repo)?
        .list()?
        .into_iter()
        .map(|backup| DbBackup {
            filename: backup.filename,
            created_at: backup.created_at,
            size_bytes: backup.size_bytes,
        })
        .collect();
    Ok(Json(backups))
}
