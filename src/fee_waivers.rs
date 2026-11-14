//! Fee waivers of clubs, riders and acts (#253, see the glossary's Fee
//! waiver): always in full and with a reason, stored as the reason in a
//! `fee_waiver` column. Granting and revoking a waiver writes a payment log
//! entry in the same transaction.

use crate::ids::{ActId, ClubId, RiderId};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::bookings::{self, Actor, BookingAction};

/// What a waiver applies to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WaiverLevel {
    /// All the club's fees: start fees and jury fee.
    Club,
    /// All the rider's starts.
    Rider,
    /// The act's start, for every rider.
    Act,
}

/// The target of a waiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Club(ClubId),
    Rider(RiderId),
    Act(ActId),
}

impl Target {
    #[must_use]
    pub fn level(self) -> WaiverLevel {
        match self {
            Target::Club(_) => WaiverLevel::Club,
            Target::Rider(_) => WaiverLevel::Rider,
            Target::Act(_) => WaiverLevel::Act,
        }
    }

    #[must_use]
    pub fn id(self) -> Uuid {
        match self {
            Target::Club(id) => id.0,
            Target::Rider(id) => id.0,
            Target::Act(id) => id.0,
        }
    }
}

/// A club a waiver concerns, by its name at the time.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct WaiverClub {
    pub id: ClubId,
    pub name: String,
}

/// A waiver as the payment log records it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct FeeWaiver {
    pub level: WaiverLevel,
    pub target_id: Uuid,
    /// The club's or rider's name, or the act's name (its riders' names
    /// without one), at the time.
    pub name: String,
    /// The clubs whose fees it changes: the club itself, the rider's club or
    /// the clubs of the act's riders.
    pub clubs: Vec<WaiverClub>,
    pub reason: String,
}

/// Why waiving failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("Bitte einen Grund angeben, warum die Gebühr erlassen wird.")]
    ReasonMissing,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Waives the fees of `target` with `reason`, or charges them again with
/// `None`. Writes a log entry only if that changes anything.
pub async fn set_waiver(
    tx: &mut SqliteConnection,
    target: Target,
    reason: Option<&str>,
    actor: &Actor<'_>,
) -> Result<(), Error> {
    let reason = match reason.map(str::trim) {
        Some("") => return Err(Error::ReasonMissing),
        reason => reason,
    };
    let id = target.id();
    let current = match target {
        Target::Club(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM clubs WHERE id = $1", id)
                .fetch_optional(&mut *tx)
                .await?
        }
        Target::Rider(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM rider WHERE id = $1", id)
                .fetch_optional(&mut *tx)
                .await?
        }
        Target::Act(_) => {
            sqlx::query_scalar!("SELECT fee_waiver FROM acts WHERE id = $1", id)
                .fetch_optional(&mut *tx)
                .await?
        }
    }
    .ok_or(Error::NotFound)?;
    if current.as_deref() == reason {
        return Ok(());
    }
    match target {
        Target::Club(_) => {
            sqlx::query!("UPDATE clubs SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *tx)
                .await?;
        }
        Target::Rider(_) => {
            sqlx::query!("UPDATE rider SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *tx)
                .await?;
        }
        Target::Act(_) => {
            sqlx::query!("UPDATE acts SET fee_waiver = $1 WHERE id = $2", reason, id)
                .execute(&mut *tx)
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
        name: name_of(tx, target).await?,
        clubs: clubs_of(tx, target).await?,
        reason: logged_reason,
    };
    bookings::log_waiver(tx, actor, action, &waiver).await?;
    Ok(())
}

async fn name_of(tx: &mut SqliteConnection, target: Target) -> sqlx::Result<String> {
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

async fn clubs_of(tx: &mut SqliteConnection, target: Target) -> sqlx::Result<Vec<WaiverClub>> {
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
