use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::startlist::StartlistAccess};

pub use db_core::acts::StartlistAct;

/// The public startlist: all acts with a category, in start order.
///
/// Public once an admin published it (403 before, except for the organizer
/// team), so it leaves out the riders' ages and club ids.
#[utoipa::path(
    get,
    tags=["query", "acts"],
    path="/startlist",
    responses(
        (status=200, content_type="application/json", body=Vec<StartlistAct>),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn startlist(
    _: StartlistAccess,
    Extension(db): Extension<DbHandle>,
) -> Result<Json<Vec<StartlistAct>>, HttpError> {
    Ok(Json(db.list_startlist().await?))
}
