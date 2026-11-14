use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

pub use db_core::groups::GroupSummary;

/// List all groups.
///
/// Lets clubs pick an existing group to join. Contains no rider data.
#[utoipa::path(
    get,
    tags=["query", "group"],
    path="/list_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<GroupSummary>),
        (status=401, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_groups(
    Extension(db): Extension<DbHandle>,
    _auth: Auth,
) -> Result<Json<Vec<GroupSummary>>, HttpError> {
    Ok(Json(db.list_groups().await?))
}
