use std::sync::Arc;

use axum::{Extension, Json, extract::Query};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{
        ClientError, HttpError, HttpServerOptions, extractor::auth::Granted,
        routes::db_backups::backups,
    },
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
    pub missing_songs: Vec<db_core::acts::MissingSong>,
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
        (status=501, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, options))]
pub async fn check_db_backup(
    Extension(repo): Extension<DbHandle>,
    Extension(options): Extension<Arc<HttpServerOptions>>,
    _: Granted<permit::ManageBackups>,
    Query(query): Query<CheckDbBackupQuery>,
) -> Result<Json<CheckDbBackupResponse>, HttpError> {
    let check = backups(&repo)?
        .check(&query.filename, &options.data_path)
        .await?;
    Ok(Json(CheckDbBackupResponse {
        blocking: check.blocking,
        pending_migrations: check.pending_migrations,
        missing_songs: check.missing_songs,
    }))
}
