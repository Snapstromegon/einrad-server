//! Bookings: the money on the event account as the organizer records it
//! (#248), and the payment log of every change to them (#249).
//!
//! A booking is a payment of one club, unassigned or ignored; a club's paid
//! amount is the sum of its payments. Every command that changes a booking
//! writes one log entry with the booking before and after, in the same
//! transaction.

use std::collections::HashMap;

use time::{Date, OffsetDateTime};

use crate::{
    DbError,
    clubs::RegistrationError,
    fee_waivers::{FeeWaiver, Target},
    ids::{BookingId, ClubId, UserId},
    roles::Role,
};

time::serde::format_description!(pub iso_date, Date, "[year]-[month]-[day]");

/// Amounts below half a cent count as zero.
const CENT: f64 = 0.005;

/// `amount` rounded to whole cents, against float sums like 0.1 + 0.2.
#[must_use]
pub fn round_cents(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

/// A booking as stored, with the name of its club.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct Booking {
    /// Negative for money going out, e.g. a refund.
    pub amount: f64,
    #[serde(with = "iso_date")]
    pub date: Date,
    pub note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    pub club_id: Option<ClubId>,
    /// The club's name; in the log, the name at the time of the change.
    pub club_name: Option<String>,
    /// Has nothing to do with the event, e.g. account fees.
    pub ignored: bool,
}

/// A booking as the organizer enters it.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct BookingInput {
    /// Negative for money going out, e.g. a refund; not 0.
    pub amount: f64,
    #[serde(with = "iso_date")]
    pub date: Date,
    /// E.g. the transfer's reference.
    #[serde(default)]
    pub note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    pub club_id: Option<ClubId>,
    /// Has nothing to do with the event; then without a club.
    #[serde(default)]
    pub ignored: bool,
}

impl BookingInput {
    /// The input with the amount in whole cents and the note trimmed, or what
    /// is wrong with it.
    pub fn checked(mut self) -> Result<Self, &'static str> {
        if !self.amount.is_finite() || self.amount.abs() < CENT {
            return Err("Bitte einen Betrag ungleich 0 eingeben.");
        }
        if self.ignored && self.club_id.is_some() {
            return Err("Eine ignorierte Buchung gehört zu keinem Verein.");
        }
        self.amount = round_cents(self.amount);
        self.note = self.note.trim().to_string();
        Ok(self)
    }
}

/// What a change did to a booking.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum BookingAction {
    Created,
    /// Amount, date or note changed (and maybe the assignment with them).
    Edited,
    /// Assigned to a club, or to another club.
    Assigned,
    Unassigned,
    Ignored,
    Deleted,
    /// Created by the migration from the paid total the club had before, or
    /// a waiver taken over from before waivers had a reason.
    TakenOver,
    /// A fee waiver granted (see [`crate::fee_waivers`]).
    Waived,
    /// A fee waiver revoked: the fees are charged again.
    WaiverRevoked,
}

impl BookingAction {
    /// The action that turns `before` into `after`: a change of only the
    /// assignment is named after it, anything else is an edit.
    #[must_use]
    pub fn of_edit(before: &Booking, after: &Booking) -> Self {
        let same_content = (before.amount - after.amount).abs() < CENT
            && before.date == after.date
            && before.note == after.note;
        if !same_content {
            BookingAction::Edited
        } else if after.ignored {
            BookingAction::Ignored
        } else if after.club_id.is_some() {
            BookingAction::Assigned
        } else {
            BookingAction::Unassigned
        }
    }
}

/// Who changes a booking: the user and their roles at the time.
#[derive(Debug, Clone)]
pub struct Actor<'a> {
    pub user_id: UserId,
    pub name: &'a str,
    pub roles: &'a [Role],
}

/// One entry of the payment log: a change to a booking or to a fee waiver.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct BookingLogEntry {
    /// `null` for a waiver.
    pub booking_id: Option<BookingId>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// `null` for changes without a user, like the migration's.
    pub user_id: Option<UserId>,
    pub user_name: Option<String>,
    /// The user's roles at the time.
    pub roles: Vec<Role>,
    pub action: BookingAction,
    /// `null` for a new booking.
    pub before: Option<Booking>,
    /// `null` for a deleted booking.
    pub after: Option<Booking>,
    /// The waiver granted or revoked; `null` for a booking.
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

/// One payment of a club, as the club sees it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Payment {
    pub id: BookingId,
    /// Negative for a refund.
    pub amount: f64,
    #[serde(with = "iso_date")]
    pub date: Date,
    pub note: String,
}

/// A booking in the organizer's list.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ListedBooking {
    pub id: BookingId,
    /// Negative for money going out, e.g. a refund.
    pub amount: f64,
    #[serde(with = "iso_date")]
    pub date: Date,
    pub note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    pub club_id: Option<ClubId>,
    pub club_name: Option<String>,
    pub ignored: bool,
}

#[async_trait::async_trait]
pub trait BookingsRepo {
    /// Records a booking and its log entry. `NotFound` if the club doesn't
    /// exist. `input` is already [`BookingInput::checked`].
    async fn create_booking(
        &self,
        actor: &Actor<'_>,
        input: &BookingInput,
    ) -> Result<BookingId, RegistrationError>;

    /// Changes a booking; logs only if something changed. `NotFound` for an
    /// unknown booking or club.
    async fn edit_booking(
        &self,
        actor: &Actor<'_>,
        id: BookingId,
        input: &BookingInput,
    ) -> Result<(), RegistrationError>;

    /// Deletes a booking; the log keeps it. `NotFound` for an unknown one.
    async fn delete_booking(
        &self,
        actor: &Actor<'_>,
        id: BookingId,
    ) -> Result<(), RegistrationError>;

    /// All bookings, newest first.
    async fn list_bookings(&self) -> Result<Vec<ListedBooking>, DbError>;

    /// The payment log, newest first.
    async fn booking_log(&self) -> Result<Vec<BookingLogEntry>, DbError>;

    /// What every club has paid: the sum of its payments. Clubs without
    /// payments are missing.
    async fn paid_by_club(&self) -> Result<HashMap<ClubId, f64>, DbError>;

    /// The payments of a club, oldest first.
    async fn payments_of_club(&self, club: ClubId) -> Result<Vec<Payment>, DbError>;

    /// Waives the fees of `target` with `reason`, or charges them again with
    /// `None`. Writes a log entry only if that changes anything. `NotFound`
    /// for an unknown target, `Rejected` for a blank reason.
    async fn set_fee_waiver(
        &self,
        actor: &Actor<'_>,
        target: Target,
        reason: Option<&str>,
    ) -> Result<(), RegistrationError>;
}
