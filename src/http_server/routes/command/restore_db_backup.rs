use std::sync::Arc;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    db_backup,
    db_restore::{self, MissingSong, RestoreError},
    http_server::{
        ClientError, HttpError, HttpServerOptions, extractor::auth::Granted,
        routes::db_backups::db_file,
    },
    normalize,
    player::PlayerHub,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreDbBackupResponse {
    /// The backup of the state before the restore; restoring it undoes this.
    pub snapshot_filename: String,
    /// Acts whose song file isn't in the data directory.
    pub missing_songs: Vec<MissingSong>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreDbBackupBody {
    /// A file name from `list_db_backups`.
    filename: String,
    /// Confirms applying the pending migrations of an older backup.
    #[serde(default)]
    force: bool,
}

/// Restore the database from a backup.
///
/// Admins only (the reload token doesn't count). Checks the backup again:
/// a corrupt backup, one without an Admin user and one from a newer release
/// are always refused (409); one with an older schema needs `force` (409
/// listing the migrations). Refused while music plays. The current state is
/// backed up first; if anything fails after the swap began, it is put back.
/// Missing songs don't block, they are listed in the response.
#[utoipa::path(
    post,
    tags=["command", "admin"],
    path="/restore_db_backup",
    request_body=RestoreDbBackupBody,
    responses(
        (status=200, content_type="application/json", body=RestoreDbBackupResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, options, player_hub))]
pub async fn restore_db_backup(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(options): Extension<Arc<HttpServerOptions>>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    auth: Granted<permit::ManageUsers>,
    Json(body): Json<RestoreDbBackupBody>,
) -> Result<Json<RestoreDbBackupResponse>, HttpError> {
    let path = db_backup::resolve_backup(&db_file(&db)?, &body.filename)?;
    if player_hub.is_playing() {
        return Err(HttpError::Conflict("Wiedergabe zuerst stoppen.".into()));
    }
    let restored = match db_restore::restore(&db, &path, body.force).await {
        Ok(restored) => restored,
        Err(RestoreError::Blocking(reasons)) => {
            return Err(HttpError::Conflict(format!(
                "Das Backup kann nicht wiederhergestellt werden: {}",
                reasons.join(" ")
            )));
        }
        Err(RestoreError::PendingMigrations(names)) => {
            return Err(HttpError::Conflict(format!(
                "Das Backup hat einen älteren Stand. Beim Wiederherstellen werden diese \
                 Migrationen angewendet (Bestätigung nötig): {}",
                names.join(", ")
            )));
        }
        Err(RestoreError::RolledBack(cause)) => {
            warn!("Restoring {} failed, rolled back: {cause}", body.filename);
            return Err(HttpError::Conflict(
                "Die Wiederherstellung ist fehlgeschlagen. Der vorherige Stand wurde wieder \
                 eingesetzt."
                    .into(),
            ));
        }
        Err(e) => return Err(e.into()),
    };
    let snapshot_filename = restored
        .snapshot
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    player_hub.reset_after_restore().await;
    if let Err(e) = normalize::requeue_missing(&db, &options.data_path).await {
        warn!("Couldn't queue the normalization after the restore: {e}");
    }
    let pool = db.get().await.clone();
    let missing_songs = db_restore::missing_songs(&pool, &options.data_path).await?;

    info!(
        "{} restored database backup {} (state before: {snapshot_filename})",
        auth.name, body.filename
    );
    if !missing_songs.is_empty() {
        warn!(
            "Restored {}: {} song file(s) are missing",
            body.filename,
            missing_songs.len()
        );
    }
    Ok(Json(RestoreDbBackupResponse {
        snapshot_filename,
        missing_songs,
    }))
}
