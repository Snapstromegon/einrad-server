use axum::{Extension, Json};
use db_core::{DbHandle, bookings::ListedBooking};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// List all bookings, newest first: payments, unassigned and ignored ones.
/// Treasurer.
#[utoipa::path(
    get,
    tags=["query", "payment"],
    path="/list_bookings",
    responses(
        (status=200, content_type="application/json", body=Vec<ListedBooking>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_bookings(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManagePayments>,
) -> Result<Json<Vec<ListedBooking>>, HttpError> {
    Ok(Json(repo.list_bookings().await?))
}
