//! The admin's database backups (#353): the download route, and what the
//! list/create/delete commands share. The backups are a capability of the
//! database backend (ADR 0019); a backend without it answers 501.

use axum::{
    Extension,
    extract::{Path, Request},
    http::{
        HeaderValue,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION},
    },
    response::{IntoResponse, Response},
};
use db_core::{
    DbHandle,
    backups::{Backups, BackupsError},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    http_server::{HttpError, extractor::auth::Granted},
    roles::permit,
};

/// The backup capability of the database backend.
///
/// # Errors
/// 501 (German message) if the backend has no file backups.
pub fn backups(repo: &DbHandle) -> Result<&dyn Backups, HttpError> {
    repo.backups().ok_or_else(|| {
        HttpError::NotImplemented(
            "Dieses Datenbank-Backend unterstützt keine Backups aus der Anwendung.".into(),
        )
    })
}

impl From<BackupsError> for HttpError {
    fn from(e: BackupsError) -> Self {
        match e {
            BackupsError::NotAFile => HttpError::Conflict(
                "Die Datenbank liegt nicht in einer Datei, Backups sind nicht möglich.".into(),
            ),
            BackupsError::NoSuchBackup(_) => HttpError::NotFound,
            BackupsError::AlreadyExists => HttpError::Conflict(
                "In dieser Sekunde wurde schon ein Backup erstellt. Bitte kurz warten.".into(),
            ),
            BackupsError::Blocking(reasons) => HttpError::Conflict(format!(
                "Das Backup kann nicht wiederhergestellt werden: {}",
                reasons.join(" ")
            )),
            BackupsError::Playing => HttpError::Conflict("Wiedergabe zuerst stoppen.".into()),
            BackupsError::PendingMigrations(names) => HttpError::Conflict(format!(
                "Das Backup hat einen älteren Stand. Beim Wiederherstellen werden diese \
                 Migrationen angewendet (Bestätigung nötig): {}",
                names.join(", ")
            )),
            BackupsError::RolledBack(cause) => {
                tracing::warn!("Restoring a database backup failed, rolled back: {cause}");
                HttpError::Conflict(
                    "Die Wiederherstellung ist fehlgeschlagen. Der vorherige Stand wurde wieder \
                     eingesetzt."
                        .into(),
                )
            }
            BackupsError::Failed(e) => {
                tracing::error!("Database backup failed: {e}");
                HttpError::InternalServerError
            }
        }
    }
}

/// Streams the backup `filename` for download. Admins only; the name must
/// be an existing backup (BAKUI-4).
pub async fn download(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageBackups>,
    Path(filename): Path<String>,
    request: Request,
) -> Result<Response, HttpError> {
    let path = backups(&repo)?.backup_path(&filename)?;
    let Ok(mut response) = ServeFile::new(path).oneshot(request).await;
    let headers = response.headers_mut();
    // The name is validated, so it is plain ASCII without quotes.
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response.into_response())
}
