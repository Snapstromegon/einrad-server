//! `PaymentCommands` and `PaymentQueries` for SQLite: bookings, the payment
//! log and fee waivers. One write transaction per command, one read
//! transaction per query.

use std::collections::HashMap;

use db_core::{
    BookingsRepo, DbError,
    actor::Actor,
    bookings::{Booking, BookingAction, BookingInput, Payment, round_cents},
    commands::payment::{CreateBooking, DeleteBooking, EditBooking, PaymentCommands, SetFeeWaived},
    error::{CommandError, QueryError},
    fee_waivers::{FeeWaiver, Target, WaiverClub, check_waiver_reason},
    ids::{BookingId, ClubId, UserId},
    queries::payment::{BookingLogEntry, ListBookingLog, ListedBooking, PaymentQueries},
};
use sqlx::SqliteConnection;
use time::{Date, OffsetDateTime};
use tracing::info;

use super::{SqliteRepo, read_tx, write_tx};

#[async_trait::async_trait]
impl PaymentCommands for SqliteRepo {
    async fn create_booking(
        &self,
        actor: &Actor,
        cmd: CreateBooking,
    ) -> Result<BookingId, CommandError> {
        Ok(write_tx!(self, tx, {
            create_booking(&mut tx, actor, &cmd.input).await?
        }))
    }

    async fn edit_booking(&self, actor: &Actor, cmd: EditBooking) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            edit_booking(&mut tx, actor, cmd.booking_id, &cmd.input).await?
        });
        Ok(())
    }

    async fn delete_booking(&self, actor: &Actor, cmd: DeleteBooking) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            delete_booking(&mut tx, actor, cmd.booking_id).await?
        });
        Ok(())
    }

    async fn set_fee_waived(&self, actor: &Actor, cmd: SetFeeWaived) -> Result<(), CommandError> {
        let reason = check_waiver_reason(cmd.reason.as_deref())?;
        write_tx!(self, tx, {
            set_fee_waived(&mut tx, actor, cmd.target, reason).await?
        });
        Ok(())
    }
}

#[async_trait::async_trait]
impl PaymentQueries for SqliteRepo {
    async fn list_bookings(&self, _actor: &Actor) -> Result<Vec<ListedBooking>, QueryError> {
        read_tx!(self, tx, { Ok(list_bookings(&mut tx).await?) })
    }

    async fn list_booking_log(
        &self,
        _actor: &Actor,
        query: ListBookingLog,
    ) -> Result<Vec<BookingLogEntry>, QueryError> {
        read_tx!(self, tx, {
            let mut log = booking_log(&mut tx).await?;
            if let Some(club_id) = query.club_id {
                log.retain(|entry| entry.concerns_club(club_id));
            }
            Ok(log)
        })
    }
}

#[async_trait::async_trait]
impl BookingsRepo for SqliteRepo {
    async fn paid_by_club(&self) -> Result<HashMap<ClubId, f64>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(paid_by_club(&mut conn).await?)
    }
}

async fn create_booking(
    conn: &mut SqliteConnection,
    actor: &Actor,
    input: &BookingInput,
) -> Result<BookingId, CommandError> {
    if let Some(club_id) = input.club_id
        && !club_exists(conn, club_id).await?
    {
        return Err(CommandError::NotFound);
    }
    let id = BookingId::new();
    info!(
        "Booking {id}: {} € for club {:?}",
        input.amount, input.club_id
    );
    sqlx::query!(
        "INSERT INTO bookings (id, amount, date, note, club_id, ignored) VALUES ($1, $2, $3, $4, $5, $6)",
        id,
        input.amount,
        input.date,
        input.note,
        input.club_id,
        input.ignored,
    )
    .execute(&mut *conn)
    .await?;
    let after = load_booking(conn, id).await?;
    log_change(
        conn,
        id,
        actor,
        BookingAction::Created,
        None,
        after.as_ref(),
    )
    .await?;
    Ok(id)
}

async fn edit_booking(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: BookingId,
    input: &BookingInput,
) -> Result<(), CommandError> {
    let before = load_booking(conn, id)
        .await?
        .ok_or(CommandError::NotFound)?;
    if let Some(club_id) = input.club_id
        && !club_exists(conn, club_id).await?
    {
        return Err(CommandError::NotFound);
    }
    info!("Editing booking {id}");
    sqlx::query!(
        "UPDATE bookings SET amount = $1, date = $2, note = $3, club_id = $4, ignored = $5 WHERE id = $6",
        input.amount,
        input.date,
        input.note,
        input.club_id,
        input.ignored,
        id,
    )
    .execute(&mut *conn)
    .await?;
    let after = load_booking(conn, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    // Saving the form unchanged changes nothing to log.
    if after != before {
        log_change(
            conn,
            id,
            actor,
            BookingAction::of_edit(&before, &after),
            Some(&before),
            Some(&after),
        )
        .await?;
    }
    Ok(())
}

async fn delete_booking(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: BookingId,
) -> Result<(), CommandError> {
    let before = load_booking(conn, id)
        .await?
        .ok_or(CommandError::NotFound)?;
    info!("Deleting booking {id}");
    sqlx::query!("DELETE FROM bookings WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    log_change(conn, id, actor, BookingAction::Deleted, Some(&before), None).await?;
    Ok(())
}

async fn list_bookings(conn: &mut SqliteConnection) -> sqlx::Result<Vec<ListedBooking>> {
    sqlx::query_as!(
        ListedBooking,
        r#"
        SELECT
            bookings.id as "id!: BookingId",
            bookings.amount,
            bookings.date as "date: Date",
            bookings.note,
            bookings.club_id as "club_id: ClubId",
            clubs.name as "club_name?: String",
            bookings.ignored
        FROM bookings LEFT JOIN clubs ON clubs.id = bookings.club_id
        ORDER BY bookings.date DESC, bookings.rowid DESC
        "#
    )
    .fetch_all(&mut *conn)
    .await
}

async fn booking_log(conn: &mut SqliteConnection) -> sqlx::Result<Vec<BookingLogEntry>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            booking_id as "booking_id: BookingId",
            at as "at: OffsetDateTime",
            user_id as "user_id: UserId",
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
    .fetch_all(&mut *conn)
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

/// Waives the fees of `target` with `reason` (already checked), or charges
/// them again with `None`.
async fn set_fee_waived(
    conn: &mut SqliteConnection,
    actor: &Actor,
    target: Target,
    reason: Option<&str>,
) -> Result<(), CommandError> {
    info!("Setting the fee waiver of {target:?} to {reason:?}");
    let id = target.id();
    let current = match target {
        Target::Club(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM clubs WHERE id = $1", id)
                .fetch_optional(&mut *conn)
                .await?
        }
        Target::Rider(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM rider WHERE id = $1", id)
                .fetch_optional(&mut *conn)
                .await?
        }
        Target::Act(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM acts WHERE id = $1", id)
                .fetch_optional(&mut *conn)
                .await?
        }
    }
    .ok_or(CommandError::NotFound)?;
    if current.as_deref() == reason {
        return Ok(());
    }
    match target {
        Target::Club(_) => {
            sqlx::query!("UPDATE clubs SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *conn)
                .await?;
        }
        Target::Rider(_) => {
            sqlx::query!("UPDATE rider SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *conn)
                .await?;
        }
        Target::Act(_) => {
            sqlx::query!("UPDATE acts SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *conn)
                .await?;
        }
    }
    // Revoking logs the reason that no longer applies.
    let (action, logged_reason) = match (reason, current) {
        (Some(reason), _) => (BookingAction::Waived, reason.to_string()),
        (None, Some(previous)) => (BookingAction::WaiverRevoked, previous),
        (None, None) => return Ok(()),
    };
    let waiver = FeeWaiver {
        level: target.level(),
        target_id: id,
        name: waiver_name_of(conn, target).await?,
        clubs: waiver_clubs_of(conn, target).await?,
        reason: logged_reason,
    };
    log_waiver(conn, actor, action, &waiver).await?;
    Ok(())
}

async fn club_exists(tx: &mut SqliteConnection, club_id: ClubId) -> sqlx::Result<bool> {
    Ok(
        sqlx::query_scalar!("SELECT 1 as one FROM clubs WHERE id = $1", club_id)
            .fetch_optional(tx)
            .await?
            .is_some(),
    )
}

/// The booking `id`, if it exists.
async fn load_booking(tx: &mut SqliteConnection, id: BookingId) -> sqlx::Result<Option<Booking>> {
    let row = sqlx::query!(
        r#"
        SELECT
            bookings.amount,
            bookings.date as "date: Date",
            bookings.note,
            bookings.club_id as "club_id: ClubId",
            clubs.name as "club_name?: String",
            bookings.ignored
        FROM bookings LEFT JOIN clubs ON clubs.id = bookings.club_id
        WHERE bookings.id = $1
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

/// Writes the log entry for a change of booking `id` from `before` to
/// `after`.
async fn log_change(
    tx: &mut SqliteConnection,
    id: BookingId,
    actor: &Actor,
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
    let roles = serde_json::to_string(&actor.roles).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let at = OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO booking_log (booking_id, at, user_id, user_name, roles, action, before, after)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        id,
        at,
        actor.user_id,
        &actor.name,
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
async fn log_waiver(
    tx: &mut SqliteConnection,
    actor: &Actor,
    action: BookingAction,
    waiver: &FeeWaiver,
) -> Result<(), sqlx::Error> {
    let waiver = serde_json::to_string(waiver).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let roles = serde_json::to_string(&actor.roles).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let at = OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO booking_log (at, user_id, user_name, roles, action, waiver)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        at,
        actor.user_id,
        &actor.name,
        roles,
        action,
        waiver,
    )
    .execute(tx)
    .await?;
    Ok(())
}

async fn waiver_name_of(tx: &mut SqliteConnection, target: Target) -> sqlx::Result<String> {
    let id = target.id();
    match target {
        Target::Club(_) => {
            sqlx::query_scalar!("SELECT name FROM clubs WHERE id = $1", id)
                .fetch_one(tx)
                .await
        }
        Target::Rider(_) => sqlx::query_scalar!(
            r#"SELECT firstname || ' ' || lastname as "name!: String" FROM rider WHERE id = $1"#,
            id
        )
        .fetch_one(tx)
        .await,
        Target::Act(_) => {
            sqlx::query_scalar!(
                r#"
                SELECT COALESCE(
                    NULLIF(acts.name, ''),
                    acts.group_name,
                    (
                        SELECT group_concat(s.firstname || ' ' || s.lastname, ' & ')
                        FROM act_participants p JOIN rider s ON s.id = p.rider_id
                        WHERE p.act_id = acts.id
                    ),
                    ''
                ) as "name!: String"
                FROM acts WHERE id = $1
                "#,
                id
            )
            .fetch_one(tx)
            .await
        }
    }
}

async fn waiver_clubs_of(
    tx: &mut SqliteConnection,
    target: Target,
) -> sqlx::Result<Vec<WaiverClub>> {
    let id = target.id();
    let rows = match target {
        Target::Club(_) => {
            sqlx::query_as!(
                WaiverClub,
                r#"SELECT id as "id!: ClubId", name FROM clubs WHERE id = $1"#,
                id
            )
            .fetch_all(tx)
            .await?
        }
        Target::Rider(_) => {
            sqlx::query_as!(
                WaiverClub,
                r#"
                SELECT clubs.id as "id!: ClubId", clubs.name
                FROM rider JOIN clubs ON clubs.id = rider.club_id
                WHERE rider.id = $1
                "#,
                id
            )
            .fetch_all(tx)
            .await?
        }
        Target::Act(_) => {
            sqlx::query_as!(
                WaiverClub,
                r#"
                SELECT DISTINCT clubs.id as "id!: ClubId", clubs.name
                FROM act_participants p
                JOIN rider s ON s.id = p.rider_id
                JOIN clubs ON clubs.id = s.club_id
                WHERE p.act_id = $1
                ORDER BY clubs.name
                "#,
                id
            )
            .fetch_all(tx)
            .await?
        }
    };
    Ok(rows)
}

/// What every club has paid; clubs without payments are missing.
pub(crate) async fn paid_by_club(
    conn: &mut SqliteConnection,
) -> sqlx::Result<HashMap<ClubId, f64>> {
    let rows = sqlx::query!(
        r#"
        SELECT club_id as "club_id!: ClubId", SUM(amount) as "paid!: f64"
        FROM bookings
        WHERE club_id IS NOT NULL
        GROUP BY club_id
        "#
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.club_id, round_cents(row.paid)))
        .collect())
}

/// The payments of a club, oldest first.
pub(crate) async fn payments_of_club(
    conn: &mut SqliteConnection,
    club: ClubId,
) -> sqlx::Result<Vec<Payment>> {
    sqlx::query_as!(
        Payment,
        r#"
        SELECT id as "id!: BookingId", amount, date as "date: Date", note
        FROM bookings
        WHERE club_id = $1
        ORDER BY date, rowid
        "#,
        club
    )
    .fetch_all(&mut *conn)
    .await
}
