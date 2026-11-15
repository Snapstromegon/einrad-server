use crate::ids::{BookingId, ClubId, UserId};
use axum::{Extension, Json, extract::Query};
use db_core::{
    prelude::*,
    queries::payment::{BookingLogEntry as BookingLogRow, ListBookingLog},
};
use time::OffsetDateTime;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::{Role, permit},
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListBookingLogQuery {
    /// Only changes to payments of this club, before or after the change.
    club_id: Option<ClubId>,
}

/// What a change did to a booking.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
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
    /// A fee waiver granted.
    Waived,
    /// A fee waiver revoked: the fees are charged again.
    WaiverRevoked,
}

impl From<db_core::bookings::BookingAction> for BookingAction {
    fn from(action: db_core::bookings::BookingAction) -> Self {
        use db_core::bookings::BookingAction as Db;
        match action {
            Db::Created => Self::Created,
            Db::Edited => Self::Edited,
            Db::Assigned => Self::Assigned,
            Db::Unassigned => Self::Unassigned,
            Db::Ignored => Self::Ignored,
            Db::Deleted => Self::Deleted,
            Db::TakenOver => Self::TakenOver,
            Db::Waived => Self::Waived,
            Db::WaiverRevoked => Self::WaiverRevoked,
        }
    }
}

/// A booking as stored, with the name of its club.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Booking {
    /// Negative for money going out, e.g. a refund.
    amount: f64,
    #[serde(with = "crate::bookings::iso_date")]
    date: time::Date,
    note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    club_id: Option<ClubId>,
    /// The club's name; in the log, the name at the time of the change.
    club_name: Option<String>,
    /// Has nothing to do with the event, e.g. account fees.
    ignored: bool,
}

impl From<db_core::bookings::Booking> for Booking {
    fn from(booking: db_core::bookings::Booking) -> Self {
        Self {
            amount: booking.amount,
            date: booking.date,
            note: booking.note,
            club_id: booking.club_id,
            club_name: booking.club_name,
            ignored: booking.ignored,
        }
    }
}

/// What a waiver applies to.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaiverLevel {
    /// All the club's fees: start fees and jury fee.
    Club,
    /// All the rider's starts.
    Rider,
    /// The act's start, for every rider.
    Act,
}

impl From<db_core::fee_waivers::WaiverLevel> for WaiverLevel {
    fn from(level: db_core::fee_waivers::WaiverLevel) -> Self {
        use db_core::fee_waivers::WaiverLevel as Db;
        match level {
            Db::Club => Self::Club,
            Db::Rider => Self::Rider,
            Db::Act => Self::Act,
        }
    }
}

/// A club a waiver concerns, by its name at the time.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct WaiverClub {
    id: ClubId,
    name: String,
}

/// A waiver as the payment log records it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct FeeWaiver {
    level: WaiverLevel,
    target_id: Uuid,
    /// The club's or rider's name, or the act's name (its riders' names
    /// without one), at the time.
    name: String,
    /// The clubs whose fees it changes: the club itself, the rider's club or
    /// the clubs of the act's riders.
    clubs: Vec<WaiverClub>,
    reason: String,
}

impl From<db_core::fee_waivers::FeeWaiver> for FeeWaiver {
    fn from(waiver: db_core::fee_waivers::FeeWaiver) -> Self {
        Self {
            level: waiver.level.into(),
            target_id: waiver.target_id,
            name: waiver.name,
            clubs: waiver
                .clubs
                .into_iter()
                .map(|club| WaiverClub {
                    id: club.id,
                    name: club.name,
                })
                .collect(),
            reason: waiver.reason,
        }
    }
}

/// One entry of the payment log: a change to a booking or to a fee waiver.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct BookingLogEntry {
    /// `null` for a waiver.
    booking_id: Option<BookingId>,
    #[serde(with = "time::serde::rfc3339")]
    at: OffsetDateTime,
    /// `null` for changes without a user, like the migration's.
    user_id: Option<UserId>,
    user_name: Option<String>,
    /// The user's roles at the time.
    roles: Vec<Role>,
    action: BookingAction,
    /// `null` for a new booking.
    before: Option<Booking>,
    /// `null` for a deleted booking.
    after: Option<Booking>,
    /// The waiver granted or revoked; `null` for a booking.
    waiver: Option<FeeWaiver>,
}

impl From<BookingLogRow> for BookingLogEntry {
    fn from(entry: BookingLogRow) -> Self {
        Self {
            booking_id: entry.booking_id,
            at: entry.at,
            user_id: entry.user_id,
            user_name: entry.user_name,
            roles: entry.roles,
            action: entry.action.into(),
            before: entry.before.map(Into::into),
            after: entry.after.map(Into::into),
            waiver: entry.waiver.map(Into::into),
        }
    }
}

/// The payment log: every change to a booking with the values before and
/// after, newest first. Treasurer.
#[utoipa::path(
    get,
    tags=["query", "payment"],
    path="/list_booking_log",
    params(ListBookingLogQuery),
    responses(
        (status=200, content_type="application/json", body=Vec<BookingLogEntry>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_booking_log(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManagePayments>,
    Query(query): Query<ListBookingLogQuery>,
) -> Result<Json<Vec<BookingLogEntry>>, HttpError> {
    let log = repo
        .list_booking_log(
            &granted.actor(),
            ListBookingLog {
                club_id: query.club_id,
            },
        )
        .await?;
    Ok(Json(log.into_iter().map(Into::into).collect()))
}
