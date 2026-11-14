use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

pub use db_core::riders::PairRequest;

/// The pair requests of all clubs, to link pairs across clubs.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_pair_requests",
    responses(
        (status=200, content_type="application/json", body=Vec<PairRequest>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_pair_requests(
    Extension(db): Extension<DbHandle>,
    _: Granted<permit::ViewRiders>,
) -> Result<Json<Vec<PairRequest>>, HttpError> {
    Ok(Json(db.list_pair_requests().await?))
}
