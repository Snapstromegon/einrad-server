//! The fee rules: what clubs owe. Pool-level helpers behind
//! [`db_core::FeesRepo`] and the club mails.

use std::collections::{HashMap, HashSet};

use db_core::{
    fees::{AmountDue, ClubFees},
    ids::ClubId,
    jury::{JuryObligation, counted_deployments, required_deployments},
};
use sqlx::{SqliteConnection, SqlitePool};

/// The fee rules of the current competition.
struct FeeRules {
    fee_per_start: f64,
    max_fee_per_rider: Option<f64>,
    /// Only with the jury rules configured.
    jury: Option<JuryFeeRules>,
}

struct JuryFeeRules {
    deployments_per_two_acts: i64,
    missing_deployment_fee: f64,
}

/// The current competition's fee rules; `None` without a competition.
async fn fee_rules(db: &SqlitePool) -> sqlx::Result<Option<FeeRules>> {
    let row = sqlx::query!(
        r#"
        SELECT
            fee_per_start,
            max_fee_per_rider,
            jury_min_age,
            jury_deployments_per_two_acts,
            jury_missing_deployment_fee
        FROM competition
        ORDER BY start_date DESC
        LIMIT 1
        "#
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| FeeRules {
        fee_per_start: row.fee_per_start,
        max_fee_per_rider: row.max_fee_per_rider,
        jury: match (
            row.jury_min_age,
            row.jury_deployments_per_two_acts,
            row.jury_missing_deployment_fee,
        ) {
            (Some(_), Some(deployments_per_two_acts), Some(missing_deployment_fee)) => {
                Some(JuryFeeRules {
                    deployments_per_two_acts,
                    missing_deployment_fee,
                })
            }
            _ => None,
        },
    }))
}

/// The fees per club, of all clubs or only of `club_id`. The jury fee only
/// applies while the jury registration is on and the competition has jury
/// rules.
pub(crate) async fn fees(
    db: &SqlitePool,
    jury_registration: bool,
    club_id: Option<ClubId>,
) -> sqlx::Result<HashMap<ClubId, ClubFees>> {
    let rules = fee_rules(db).await?;
    let start_fees = start_fees_by_club(db, rules.as_ref(), club_id, true).await?;
    let unwaived_start_fees = start_fees_by_club(db, rules.as_ref(), club_id, false).await?;
    let waived_clubs: HashSet<ClubId> = sqlx::query_scalar!(
        r#"
        SELECT id as "id!: ClubId" FROM clubs
        WHERE fee_waiver IS NOT NULL AND (?1 IS NULL OR id = ?1)
        "#,
        club_id
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();
    let obligations = match rules.as_ref().and_then(|r| r.jury.as_ref()) {
        Some(rules) if jury_registration => {
            let mut conn = db.acquire().await?;
            obligations(&mut conn, rules, club_id).await?
        }
        _ => HashMap::new(),
    };
    let clubs: HashSet<ClubId> = unwaived_start_fees
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
    db: &SqlitePool,
    rules: Option<&FeeRules>,
    club_id: Option<ClubId>,
    waivers: bool,
) -> sqlx::Result<HashMap<ClubId, f64>> {
    let Some(rules) = rules else {
        return Ok(HashMap::new());
    };
    let riders = sqlx::query!(
        r#"
        SELECT
            rider.club_id as "club_id!: ClubId",
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
        let fee = rider.starts as f64 * rules.fee_per_start;
        let fee = rules.max_fee_per_rider.map_or(fee, |max| fee.min(max));
        *fees.entry(rider.club_id).or_insert(0.0) += fee;
    }
    Ok(fees)
}

/// The jury obligation of every club, or only of `club_id`.
///
/// An act counts when it starts: it has a category, it's a
/// group with regular riders (also an incomplete one), or it was withdrawn
/// without the fee being waived. Fee waivers of starting acts, riders and
/// clubs don't lower it. It counts for its owner club: the
/// group's creator, or the club of a single's or pair's riders. A trained
/// judge counts the deployments of every jury unit they registered for
/// as a technical or performance judge (once per unit), at most
/// `max_deployments`. Dismount-only registrations do not count.
async fn obligations(
    conn: &mut SqliteConnection,
    rules: &JuryFeeRules,
    club_id: Option<ClubId>,
) -> sqlx::Result<HashMap<ClubId, JuryObligation>> {
    let clubs = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: ClubId",
            clubs.jury_deployments_override,
            (
                SELECT COUNT(*) FROM view_act AS a
                WHERE NOT (a.withdrawn_at IS NOT NULL AND a.fee_waiver IS NOT NULL)
                    AND (
                        a.category IS NOT NULL
                        OR a.withdrawn_at IS NOT NULL
                        -- Incomplete groups pay start fees, too.
                        OR (a.act_type IN ('group_small', 'group_large') AND a.regular_count > 0)
                    )
                    AND COALESCE(
                        a.owner_club_id,
                        (
                            -- A pair across clubs (only admins link those)
                            -- counts for one of them.
                            SELECT MIN(s.club_id) FROM act_participants p
                            JOIN rider s ON s.id = p.rider_id
                            WHERE p.act_id = a.id AND NOT p.is_replacement
                        )
                    ) = clubs.id
            ) as "acts!: i64"
        FROM clubs
        WHERE ?1 IS NULL OR clubs.id = ?1
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let judges = sqlx::query!(
        r#"
        SELECT
            judge.club_id as "club_id!: ClubId",
            judge.max_deployments,
            (
                SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                WHERE c.id IN (
                    SELECT unit_id FROM judge_registration
                    WHERE judge_id = judge.id AND task != 'dismounts'
                )
            ) as "deployments!: i64"
        FROM judge
        WHERE judge.is_trained AND (?1 IS NULL OR judge.club_id = ?1)
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut registered: HashMap<ClubId, i64> = HashMap::new();
    for judge in judges {
        let deployments = counted_deployments(true, judge.deployments, judge.max_deployments);
        *registered.entry(judge.club_id).or_default() += deployments;
    }
    Ok(clubs
        .into_iter()
        .map(|club| {
            let required = required_deployments(club.acts, rules.deployments_per_two_acts);
            let registered = registered.get(&club.id).copied().unwrap_or(0);
            let credited = club.jury_deployments_override.unwrap_or(registered);
            let missing = (required - credited).max(0);
            let obligation = JuryObligation {
                acts: club.acts,
                required,
                registered,
                credited_override: club.jury_deployments_override,
                credited,
                missing,
                fee: missing as f64 * rules.missing_deployment_fee,
            };
            (club.id, obligation)
        })
        .collect())
}
