use axum::{Extension, Json};
use db_core::{DbHandle, heats::Heat};
use tracing::instrument;

use crate::http_server::{ClientError, HttpError};

/// List the heats: groups of categories of one discipline whose best acts
/// fill a final's slots. In the order of their first category; heats without
/// categories last.
#[utoipa::path(
    get,
    tags=["query", "heat"],
    path="/list_heats",
    responses(
        (status=200, content_type="application/json", body=Vec<Heat>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_heats(
    Extension(repo): Extension<DbHandle>,
) -> Result<Json<Vec<Heat>>, HttpError> {
    Ok(Json(repo.list_heats().await?))
}
