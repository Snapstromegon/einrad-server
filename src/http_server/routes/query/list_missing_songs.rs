use std::sync::Arc;

use axum::{Extension, Json};
use db_core::{DbHandle, acts::MissingSong};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    roles::permit,
};

/// List the acts whose song file is missing from the data directory ("Songs
/// prüfen"), e.g. after a restore: the songs aren't part of a backup.
///
/// Admins only.
#[utoipa::path(
    get,
    tags=["query", "admin"],
    path="/list_missing_songs",
    responses(
        (status=200, content_type="application/json", body=Vec<MissingSong>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, options))]
pub async fn list_missing_songs(
    Extension(db): Extension<DbHandle>,
    Extension(options): Extension<Arc<HttpServerOptions>>,
    _: Granted<permit::ManageBackups>,
) -> Result<Json<Vec<MissingSong>>, HttpError> {
    Ok(Json(db_core::acts::missing_songs(
        db.list_act_songs().await?,
        &options.data_path,
    )))
}
