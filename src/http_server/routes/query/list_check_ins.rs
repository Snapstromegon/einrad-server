use axum::{Extension, Json};
use db_core::{DbHandle, check_in::CheckIn};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// The check-in state of every rider. Check-in and music control.
///
/// Small enough to poll during the competition.
#[utoipa::path(
    get,
    tags=["query", "check-in"],
    path="/list_check_ins",
    responses(
        (status=200, content_type="application/json", body=Vec<CheckIn>),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_check_ins(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::CheckIn>,
) -> Result<Json<Vec<CheckIn>>, HttpError> {
    Ok(Json(repo.list_check_ins().await?))
}
