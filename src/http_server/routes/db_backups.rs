//! The admin's database backups (#353): the download route, and what the
//! list/create/delete commands share.

use std::path::PathBuf;

use axum::{
    Extension,
    extract::{Path, Request},
    http::{
        HeaderValue,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION},
    },
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    db_backup::{self, BackupError},
    http_server::{HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// The live database file, derived from the configured database URL.
/// Backups sit next to it.
///
/// # Errors
/// A conflict (German message) if the database isn't a file, e.g. in memory.
pub fn db_file(db: &ReloadableSqlite) -> Result<PathBuf, HttpError> {
    db.file_path().ok_or_else(|| {
        HttpError::Conflict(
            "Die Datenbank liegt nicht in einer Datei, Backups sind nicht möglich.".into(),
        )
    })
}

impl From<BackupError> for HttpError {
    fn from(e: BackupError) -> Self {
        match e {
            BackupError::NoSuchBackup(_) => HttpError::NotFound,
            e => {
                tracing::error!("Database backup failed: {e}");
                HttpError::InternalServerError
            }
        }
    }
}

/// Streams the backup `filename` for download. Admins only; the name must
/// be an existing backup (BAKUI-4).
pub async fn download(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageUsers>,
    Path(filename): Path<String>,
    request: Request,
) -> Result<Response, HttpError> {
    let path = db_backup::resolve_backup(&db_file(&db)?, &filename)?;
    let Ok(mut response) = ServeFile::new(path).oneshot(request).await;
    let headers = response.headers_mut();
    // The name is validated, so it is plain ASCII without quotes.
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response.into_response())
}
