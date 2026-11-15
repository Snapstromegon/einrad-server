use axum::{Extension, Json};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    db_backup,
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::db_file},
    reloadable_sqlite::ReloadableSqlite,
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
    ),
)]
#[instrument(skip(db))]
pub async fn create_db_backup(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageBackups>,
) -> Result<Json<CreateDbBackupResponse>, HttpError> {
    let target = db_backup::backup_path(&db_file(&db)?, db_backup::now_local());
    if target.exists() {
        return Err(HttpError::Conflict(
            "In dieser Sekunde wurde schon ein Backup erstellt. Bitte kurz warten.".into(),
        ));
    }
    let pool = db.get().await.clone();
    db_backup::create_backup_at(&pool, &target).await?;
    let filename = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    info!("Created database backup {filename}");
    Ok(Json(CreateDbBackupResponse { filename }))
}
