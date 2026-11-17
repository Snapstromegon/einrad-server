use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::db_backups::backups},
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
        (status=501, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn delete_db_backups(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageBackups>,
    Json(body): Json<DeleteDbBackupsBody>,
) -> Result<Json<DeleteDbBackupsResponse>, HttpError> {
    backups(&repo)?.delete(&body.filenames)?;
    Ok(Json(DeleteDbBackupsResponse {}))
}
