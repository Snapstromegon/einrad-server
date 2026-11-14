use crate::ids::BookingId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    bookings::BookingInput,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateBookingResponse {
    id: BookingId,
}

/// Record a booking: a payment of a club (a refund when negative), an
/// unassigned or an ignored booking. Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/create_booking",
    request_body=BookingInput,
    responses(
        (status=200, content_type="application/json", body=CreateBookingResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn create_booking(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<BookingInput>,
) -> Result<Json<CreateBookingResponse>, HttpError> {
    let input = body
        .checked()
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let id = repo
        .create_booking(&granted.booking_actor(), &input)
        .await?;
    Ok(Json(CreateBookingResponse { id }))
}
