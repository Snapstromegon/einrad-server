//! What clubs owe: start fees and the fee for missing jury deployments.
//!
//! The one place the fee rules live in the backend; the club queries, the
//! payment QR code (through them) and `/metrics` all use it.

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    competition::Competition,
    jury::{self, JuryObligation},
};

/// What a club owes, split into its parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct AmountDue {
    /// `fee_per_start` per start, at most `max_fee_per_rider` per rider.
    pub start_fees: f64,
    /// For missing jury deployments; 0 without the jury registration.
    pub jury_fee: f64,
    pub total: f64,
    /// What fee waivers drop: the start fees of waived clubs, riders and
    /// acts, and the jury fee of a waived club.
    pub waived: f64,
}

/// What a club owes and, with the jury registration, its jury obligation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClubFees {
    pub amount_due: AmountDue,
    pub jury: Option<JuryObligation>,
}

/// The fees of every club. Clubs without riders and jury obligation owe
/// nothing and may be missing from the map.
pub async fn fees_by_club(
    db: &sqlx::SqlitePool,
    competition: Option<&Competition>,
    jury_registration: bool,
) -> sqlx::Result<HashMap<Uuid, ClubFees>> {
    fees(db, competition, jury_registration, None).await
}

/// The fees of one club.
pub async fn fees_of_club(
    db: &sqlx::SqlitePool,
    competition: Option<&Competition>,
    jury_registration: bool,
    club_id: Uuid,
) -> sqlx::Result<ClubFees> {
    Ok(fees(db, competition, jury_registration, Some(club_id))
        .await?
        .remove(&club_id)
        .unwrap_or_default())
}

/// The fees per club, of all clubs or only of `club_id`. The jury fee only
/// applies while the jury registration is on and the competition has jury
/// rules.
async fn fees(
    db: &sqlx::SqlitePool,
    competition: Option<&Competition>,
    jury_registration: bool,
    club_id: Option<Uuid>,
) -> sqlx::Result<HashMap<Uuid, ClubFees>> {
    let start_fees = start_fees_by_club(db, competition, club_id, true).await?;
    let unwaived_start_fees = start_fees_by_club(db, competition, club_id, false).await?;
    let waived_clubs: std::collections::HashSet<Uuid> = sqlx::query_scalar!(
        r#"
        SELECT id as "id!: Uuid" FROM clubs
        WHERE fee_waiver IS NOT NULL AND (?1 IS NULL OR id = ?1)
        "#,
        club_id
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();
    let obligations = match competition.and_then(|c| c.jury.as_ref()) {
        Some(rules) if jury_registration => {
            let mut conn = db.acquire().await?;
            jury::obligations(&mut conn, rules, club_id).await?
        }
        _ => HashMap::new(),
    };
    let clubs: std::collections::HashSet<Uuid> = unwaived_start_fees
        .keys()
        .chain(obligations.keys())
        .copied()
        .collect();
    Ok(clubs
        .into_iter()
        .map(|id| {
            let start_fees = start_fees.get(&id).copied().unwrap_or(0.0);
            let unwaived = unwaived_start_fees.get(&id).copied().unwrap_or(0.0);
            let jury = obligations.get(&id).copied();
            // A waived club keeps its obligation, only the fee is gone.
            let obligation_fee = jury.map_or(0.0, |jury| jury.fee);
            let jury_fee = if waived_clubs.contains(&id) {
                0.0
            } else {
                obligation_fee
            };
            let fees = ClubFees {
                amount_due: AmountDue {
                    start_fees,
                    jury_fee,
                    total: start_fees + jury_fee,
                    waived: unwaived - start_fees + obligation_fee - jury_fee,
                },
                jury,
            };
            (id, fees)
        })
        .collect())
}

/// Start fees per club. Each start in an act counts, and a pair request (a
/// rider waiting for a partner) counts as one; the rider's club pays, at
/// most the cap per rider. With `waivers`, a waived club or rider pays
/// nothing and a waived act's start doesn't count. The
/// same rule as `getRiderPrice` in the frontend, which still prices single
/// riders in the forms.
async fn start_fees_by_club(
    db: &sqlx::SqlitePool,
    competition: Option<&Competition>,
    club_id: Option<Uuid>,
    waivers: bool,
) -> sqlx::Result<HashMap<Uuid, f64>> {
    let Some(competition) = competition else {
        return Ok(HashMap::new());
    };
    let riders = sqlx::query!(
        r#"
        SELECT
            rider.club_id as "club_id!: Uuid",
            CASE WHEN ?2 AND (
                rider.fee_waiver IS NOT NULL
                OR (SELECT fee_waiver FROM clubs WHERE clubs.id = rider.club_id) IS NOT NULL
            ) THEN 0 ELSE MAX(
                0,
                (SELECT COUNT(*) FROM act_participants WHERE act_participants.rider_id = rider.id)
                + EXISTS (SELECT 1 FROM pair_requests WHERE pair_requests.rider_id = rider.id)
                - (
                    SELECT COUNT(*) FROM act_participants
                    JOIN acts ON acts.id = act_participants.act_id
                    WHERE act_participants.rider_id = rider.id
                        AND ?2 AND acts.fee_waiver IS NOT NULL
                )
            ) END as "starts!: i64"
        FROM rider
        WHERE ?1 IS NULL OR rider.club_id = ?1
        "#,
        club_id,
        waivers
    )
    .fetch_all(db)
    .await?;
    let mut fees = HashMap::new();
    for rider in riders {
        let fee = rider.starts as f64 * competition.fee_per_start;
        let fee = competition
            .max_fee_per_rider
            .map_or(fee, |max| fee.min(max));
        *fees.entry(rider.club_id).or_insert(0.0) += fee;
    }
    Ok(fees)
}
