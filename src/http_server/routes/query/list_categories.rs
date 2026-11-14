use axum::{Extension, Json};
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};
use db_core::{DbHandle, categories::Category};

/// Get information about a club.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_categories",
    responses(
        (status=200, content_type="application/json", body=Vec<Category>),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_categories(
    Extension(repo): Extension<DbHandle>,
    auth: Option<Auth>,
) -> Result<Json<Vec<Category>>, HttpError> {
    Ok(Json(repo.list_categories().await?))
}
