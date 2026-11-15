use crate::ids::{BookingId, ClubId};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::payment::ListedBooking as ListedBookingRow};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// A booking in the organizer's list.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ListedBooking {
    id: BookingId,
    /// Negative for money going out, e.g. a refund.
    amount: f64,
    #[serde(with = "crate::bookings::iso_date")]
    date: time::Date,
    note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    club_id: Option<ClubId>,
    club_name: Option<String>,
    ignored: bool,
}

impl From<ListedBookingRow> for ListedBooking {
    fn from(booking: ListedBookingRow) -> Self {
        Self {
            id: booking.id,
            amount: booking.amount,
            date: booking.date,
            note: booking.note,
            club_id: booking.club_id,
            club_name: booking.club_name,
            ignored: booking.ignored,
        }
    }
}

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
    granted: Granted<permit::ManagePayments>,
) -> Result<Json<Vec<ListedBooking>>, HttpError> {
    let bookings = repo.list_bookings(&granted.actor()).await?;
    Ok(Json(bookings.into_iter().map(Into::into).collect()))
}
