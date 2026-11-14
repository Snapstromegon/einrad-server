use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    db_backup,
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::db_file},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteDbBackupsResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteDbBackupsBody {
    /// File names from `list_db_backups`.
    filenames: Vec<String>,
}

/// Delete database backups.
///
/// Admins only. Every name must be an existing backup; if one isn't, nothing
/// is deleted (404). The live database can never be deleted this way.
#[utoipa::path(
    post,
    tags=["command", "admin"],
    path="/delete_db_backups",
    request_body=DeleteDbBackupsBody,
    responses(
        (status=200, content_type="application/json", body=DeleteDbBackupsResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn delete_db_backups(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageBackups>,
    Json(body): Json<DeleteDbBackupsBody>,
) -> Result<Json<DeleteDbBackupsResponse>, HttpError> {
    let db_path = db_file(&db)?;
    // Check all names first, so a bad one deletes nothing.
    let paths = body
        .filenames
        .iter()
        .map(|name| db_backup::resolve_backup(&db_path, name))
        .collect::<Result<Vec<_>, _>>()?;
    for path in paths {
        match std::fs::remove_file(&path) {
            Ok(()) => info!("Deleted database backup {}", path.display()),
            // Deleted meanwhile, which is what was asked for.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::error!("Couldn't delete {}: {e}", path.display());
                return Err(HttpError::InternalServerError);
            }
        }
    }
    Ok(Json(DeleteDbBackupsResponse {}))
}
