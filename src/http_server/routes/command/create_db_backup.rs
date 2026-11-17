use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::backups},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateDbBackupResponse {
    /// The file name of the new backup.
    pub filename: String,
}

/// Back up the database now: a `VACUUM INTO` snapshot next to the database
/// file, named like the backups before migrations.
///
/// Admins only. Fails with a conflict if a backup of the same second exists.
#[utoipa::path(
    post,
    tags=["command", "admin"],
    path="/create_db_backup",
    responses(
        (status=200, content_type="application/json", body=CreateDbBackupResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
        (status=501, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn create_db_backup(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageBackups>,
) -> Result<Json<CreateDbBackupResponse>, HttpError> {
    let filename = backups(&repo)?.create().await?;
    info!("Created database backup {filename}");
    Ok(Json(CreateDbBackupResponse { filename }))
}
