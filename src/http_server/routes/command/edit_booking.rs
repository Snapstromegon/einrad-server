use crate::ids::BookingId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    bookings::BookingInput,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditBookingResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditBookingBody {
    booking_id: BookingId,
    booking: BookingInput,
}

/// Change a booking: its amount, date or note, or assign it to a club,
/// unassign or ignore it. Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/edit_booking",
    request_body=EditBookingBody,
    responses(
        (status=200, content_type="application/json", body=EditBookingResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_booking(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<EditBookingBody>,
) -> Result<Json<EditBookingResponse>, HttpError> {
    let input = body
        .booking
        .checked()
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    repo.edit_booking(&granted.booking_actor(), body.booking_id, &input)
        .await?;
    Ok(Json(EditBookingResponse {}))
}
