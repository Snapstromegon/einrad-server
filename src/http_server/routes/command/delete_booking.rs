use crate::ids::BookingId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteBookingResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBookingBody {
    booking_id: BookingId,
}

/// Delete a booking, e.g. one recorded twice; the payment log keeps it.
/// Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/delete_booking",
    request_body=DeleteBookingBody,
    responses(
        (status=200, content_type="application/json", body=DeleteBookingResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_booking(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<DeleteBookingBody>,
) -> Result<Json<DeleteBookingResponse>, HttpError> {
    repo.delete_booking(&granted.booking_actor(), body.booking_id)
        .await?;
    Ok(Json(DeleteBookingResponse {}))
}
