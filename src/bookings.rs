//! Bookings: the money on the event account as the organizer records it
//! (#248), and the payment log of every change to them (#249).
//!
//! A booking is a payment of one club, unassigned or ignored; a club's paid
//! amount is the sum of its payments. Every command that changes a booking
//! writes one log entry with the booking before and after, in the same
//! transaction.

use std::collections::HashMap;

use sqlx::{Sqlite, SqliteConnection};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{fee_waivers::FeeWaiver, http_server::extractor::auth::Auth, roles::Role};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

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
    pub club_id: Option<Uuid>,
    /// The club's name; in the log, the name at the time of the change.
    pub club_name: Option<String>,
    /// Has nothing to do with the event, e.g. account fees.
    pub ignored: bool,
}

impl Booking {
    /// The booking `id`, if it exists.
    pub async fn load(tx: &mut SqliteConnection, id: Uuid) -> sqlx::Result<Option<Self>> {
        let row = sqlx::query!(
            r#"
            SELECT
                bookings.amount,
                bookings.date as "date: Date",
                bookings.note,
                bookings.club_id as "club_id: Uuid",
                clubs.name as "club_name?: String",
                bookings.ignored
            FROM bookings LEFT JOIN clubs ON clubs.id = bookings.club_id
            WHERE bookings.id = ?
            "#,
            id
        )
        .fetch_optional(tx)
        .await?;
        Ok(row.map(|row| Booking {
            amount: row.amount,
            date: row.date,
            note: row.note,
            club_id: row.club_id,
            club_name: row.club_name,
            ignored: row.ignored,
        }))
    }
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
    pub club_id: Option<Uuid>,
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
    pub user_id: Uuid,
    pub name: &'a str,
    pub roles: &'a [Role],
}

impl<'a> Actor<'a> {
    #[must_use]
    pub fn of(auth: &'a Auth) -> Self {
        Actor {
            user_id: auth.user_id,
            name: &auth.name,
            roles: &auth.roles,
        }
    }
}

/// Writes the log entry for a change of booking `id` from `before` to
/// `after`.
pub async fn log_change(
    tx: &mut SqliteConnection,
    id: Uuid,
    actor: &Actor<'_>,
    action: BookingAction,
    before: Option<&Booking>,
    after: Option<&Booking>,
) -> Result<(), sqlx::Error> {
    let json = |booking: Option<&Booking>| {
        booking
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| sqlx::Error::Encode(e.into()))
    };
    let (before, after) = (json(before)?, json(after)?);
    let roles = serde_json::to_string(actor.roles).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let at = OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO booking_log (booking_id, at, user_id, user_name, roles, action, before, after)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        id,
        at,
        actor.user_id,
        actor.name,
        roles,
        action,
        before,
        after,
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// Writes the log entry for granting or revoking `waiver`.
pub async fn log_waiver(
    tx: &mut SqliteConnection,
    actor: &Actor<'_>,
    action: BookingAction,
    waiver: &FeeWaiver,
) -> Result<(), sqlx::Error> {
    let waiver = serde_json::to_string(waiver).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let roles = serde_json::to_string(actor.roles).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let at = OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO booking_log (at, user_id, user_name, roles, action, waiver)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        at,
        actor.user_id,
        actor.name,
        roles,
        action,
        waiver,
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// One entry of the payment log: a change to a booking or to a fee waiver.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct BookingLogEntry {
    /// `null` for a waiver.
    pub booking_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// `null` for changes without a user, like the migration's.
    pub user_id: Option<Uuid>,
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
    pub fn concerns_club(&self, club_id: Uuid) -> bool {
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

/// The payment log, newest first.
pub async fn log<'e, E>(db: E) -> Result<Vec<BookingLogEntry>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query!(
        r#"
        SELECT
            booking_id as "booking_id: Uuid",
            at as "at: OffsetDateTime",
            user_id as "user_id: Uuid",
            user_name,
            roles,
            action as "action: BookingAction",
            before,
            after,
            waiver
        FROM booking_log
        ORDER BY id DESC
        "#
    )
    .fetch_all(db)
    .await?;
    let decode = |e: serde_json::Error| sqlx::Error::Decode(e.into());
    let booking = |json: Option<String>| {
        json.map(|json| serde_json::from_str::<Booking>(&json))
            .transpose()
            .map_err(decode)
    };
    rows.into_iter()
        .map(|row| {
            Ok(BookingLogEntry {
                booking_id: row.booking_id,
                at: row.at,
                user_id: row.user_id,
                user_name: row.user_name,
                roles: serde_json::from_str(&row.roles).map_err(decode)?,
                action: row.action,
                before: booking(row.before)?,
                after: booking(row.after)?,
                waiver: row
                    .waiver
                    .map(|json| serde_json::from_str::<FeeWaiver>(&json))
                    .transpose()
                    .map_err(decode)?,
            })
        })
        .collect()
}

/// What every club has paid: the sum of its payments. Clubs without payments
/// are missing.
pub async fn paid_by_club<'e, E>(db: E) -> sqlx::Result<HashMap<Uuid, f64>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query!(
        r#"
        SELECT club_id as "club_id!: Uuid", SUM(amount) as "paid!: f64"
        FROM bookings
        WHERE club_id IS NOT NULL
        GROUP BY club_id
        "#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.club_id, round_cents(row.paid)))
        .collect())
}

/// One payment of a club, as the club sees it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Payment {
    pub id: Uuid,
    /// Negative for a refund.
    pub amount: f64,
    #[serde(with = "iso_date")]
    pub date: Date,
    pub note: String,
}

/// The payments of `club_id`, oldest first.
pub async fn payments_of_club<'e, E>(db: E, club_id: Uuid) -> sqlx::Result<Vec<Payment>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_as!(
        Payment,
        r#"
        SELECT id as "id!: Uuid", amount, date as "date: Date", note
        FROM bookings
        WHERE club_id = ?
        ORDER BY date, rowid
        "#,
        club_id
    )
    .fetch_all(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn booking(amount: f64, club_id: Option<Uuid>, ignored: bool) -> Booking {
        Booking {
            amount,
            date: date!(2027 - 03 - 01),
            note: "Überweisung".into(),
            club_id,
            club_name: None,
            ignored,
        }
    }

    #[test]
    fn an_edit_of_only_the_assignment_is_named_after_it() {
        let club = Some(Uuid::now_v7());
        let unassigned = booking(45.0, None, false);
        let assigned = booking(45.0, club, false);
        let ignored = booking(45.0, None, true);
        assert_eq!(
            BookingAction::of_edit(&unassigned, &assigned),
            BookingAction::Assigned
        );
        assert_eq!(
            BookingAction::of_edit(&assigned, &unassigned),
            BookingAction::Unassigned
        );
        assert_eq!(
            BookingAction::of_edit(&unassigned, &ignored),
            BookingAction::Ignored
        );
        assert_eq!(
            BookingAction::of_edit(&unassigned, &booking(50.0, club, false)),
            BookingAction::Edited
        );
    }
}
