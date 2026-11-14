use axum::{Extension, Json};
use db_core::{DbHandle, finals::Final};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// List the finals, in order.
#[utoipa::path(
    get,
    tags=["query", "final"],
    path="/list_finals",
    responses(
        (status=200, content_type="application/json", body=Vec<Final>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_finals(
    Extension(repo): Extension<DbHandle>,
    results: Option<Granted<permit::ManageResults>>,
) -> Result<Json<Vec<Final>>, HttpError> {
    Ok(Json(repo.list_finals(results.is_some()).await?))
}
