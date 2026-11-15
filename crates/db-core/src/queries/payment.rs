//! Payment queries: bookings and the payment log.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::{Date, OffsetDateTime};

use crate::{
    actor::Actor,
    bookings::{Booking, BookingAction},
    error::QueryError,
    fee_waivers::FeeWaiver,
    ids::{BookingId, ClubId, UserId},
    roles::Role,
};

/// The payment log, optionally of one club.
#[derive(Debug, Clone)]
pub struct ListBookingLog {
    /// Only changes to payments of this club, before or after the change.
    pub club_id: Option<ClubId>,
}

/// A booking in the organizer's list.
#[derive(Debug, Clone)]
pub struct ListedBooking {
    pub id: BookingId,
    /// Negative for money going out, e.g. a refund.
    pub amount: f64,
    pub date: Date,
    pub note: String,
    /// The club this booking is a payment of; `None` if unassigned or ignored.
    pub club_id: Option<ClubId>,
    pub club_name: Option<String>,
    pub ignored: bool,
}

/// One entry of the payment log: a change to a booking or to a fee waiver.
#[derive(Debug, Clone)]
pub struct BookingLogEntry {
    /// `None` for a waiver.
    pub booking_id: Option<BookingId>,
    pub at: OffsetDateTime,
    /// `None` for changes without a user, like the migration's.
    pub user_id: Option<UserId>,
    pub user_name: Option<String>,
    /// The user's roles at the time.
    pub roles: Vec<Role>,
    pub action: BookingAction,
    /// `None` for a new booking.
    pub before: Option<Booking>,
    /// `None` for a deleted booking.
    pub after: Option<Booking>,
    /// The waiver granted or revoked; `None` for a booking.
    pub waiver: Option<FeeWaiver>,
}

impl BookingLogEntry {
    /// Whether the booking was a payment of `club_id` before or after, or
    /// the waiver changes the club's fees.
    #[must_use]
    pub fn concerns_club(&self, club_id: ClubId) -> bool {
        [&self.before, &self.after]
            .into_iter()
            .flatten()
            .any(|b| b.club_id == Some(club_id))
            || self
                .waiver
                .as_ref()
                .is_some_and(|w| w.clubs.iter().any(|c| c.id == club_id))
    }
}

/// The read side of the payment area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait PaymentQueries: Send + Sync {
    /// All bookings, newest first.
    async fn list_bookings(&self, actor: &Actor) -> Result<Vec<ListedBooking>, QueryError>;

    /// The payment log, newest first.
    async fn list_booking_log(
        &self,
        actor: &Actor,
        query: ListBookingLog,
    ) -> Result<Vec<BookingLogEntry>, QueryError>;
}
