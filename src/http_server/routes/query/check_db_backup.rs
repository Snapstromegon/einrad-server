use std::sync::Arc;

use axum::{Extension, Json, extract::Query};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    db_backup, db_restore,
    http_server::{
        ClientError, HttpError, HttpServerOptions, extractor::auth::Granted,
        routes::db_backups::db_file,
    },
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckDbBackupQuery {
    /// A file name from `list_db_backups`.
    filename: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CheckDbBackupResponse {
    /// Why the backup can never be restored (German). Empty if it can.
    pub blocking: Vec<String>,
    /// The migrations the backup lacks (older schema), by name. Restoring
    /// needs `force` and applies them.
    pub pending_migrations: Vec<String>,
    /// Acts whose song file isn't in the data directory.
    pub missing_songs: Vec<db_restore::MissingSong>,
}

/// Check a backup before restoring it: blocking problems, pending
/// migrations and missing songs. The backup is opened read-only.
///
/// Admins only.
#[utoipa::path(
    get,
    tags=["query", "admin"],
    path="/check_db_backup",
    params(("filename" = String, Query, description = "A file name from `list_db_backups`.")),
    responses(
        (status=200, content_type="application/json", body=CheckDbBackupResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, options))]
pub async fn check_db_backup(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(options): Extension<Arc<HttpServerOptions>>,
    _: Granted<permit::ManageUsers>,
    Query(query): Query<CheckDbBackupQuery>,
) -> Result<Json<CheckDbBackupResponse>, HttpError> {
    let path = db_backup::resolve_backup(&db_file(&db)?, &query.filename)?;
    let check = db_restore::check_backup(&path, &sqlx::migrate!("./migrations")).await;
    // Songs can only be listed from a readable database.
    let missing_songs = if check.blocking.is_empty() {
        db_restore::missing_songs_in_backup(&path, &options.data_path).await?
    } else {
        vec![]
    };
    Ok(Json(CheckDbBackupResponse {
        blocking: check.blocking,
        pending_migrations: check.pending_migrations,
        missing_songs,
    }))
}
