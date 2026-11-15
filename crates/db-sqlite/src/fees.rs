//! The fee facts: counts and rows of riders, acts, judges and waivers, which
//! the pure rules of [`db_core::fees`] turn into what clubs owe. Pool-level
//! helpers behind [`db_core::FeesRepo`] and the club mails.

use std::collections::{HashMap, HashSet};

use db_core::{
    act_type::ActType,
    fees::{ClubFees, FeeFacts, FeeRules, RiderFeeFacts, club_fees},
    ids::{ClubId, JudgeId, JuryUnitId},
    jury::{
        JudgeFacts, JudgeTask, JuryActFacts, JuryClubFacts, JuryFacts, JuryFeeRules,
        JuryParticipantFacts, UnitRegistration,
    },
};
use sqlx::{SqliteConnection, SqlitePool};

/// The current competition's fee rules; `None` without a competition.
async fn fee_rules(conn: &mut SqliteConnection) -> sqlx::Result<Option<FeeRules>> {
    Ok(crate::competition::current(conn)
        .await?
        .map(|competition| FeeRules {
            fee_per_start: competition.fee_per_start,
            max_fee_per_rider: competition.max_fee_per_rider,
            jury: competition.jury.map(|jury| JuryFeeRules {
                deployments_per_two_acts: jury.deployments_per_two_acts,
                missing_deployment_fee: jury.missing_deployment_fee,
            }),
        }))
}

/// The fees per club, of all clubs or only of `club_id`: loads the facts and
/// leaves the arithmetic to [`club_fees`]. All facts are read in one read
/// transaction, so a concurrent write (a pair link, a withdrawal) can't tear
/// them apart.
pub(crate) async fn fees(
    db: &SqlitePool,
    jury_registration: bool,
    club_id: Option<ClubId>,
) -> sqlx::Result<HashMap<ClubId, ClubFees>> {
    let mut tx = db.begin().await?;
    let conn = &mut *tx;
    let rules = fee_rules(conn).await?;
    let riders = if rules.is_some() {
        rider_facts(conn, club_id).await?
    } else {
        Vec::new()
    };
    let waived_clubs: HashSet<ClubId> = sqlx::query_scalar!(
        r#"
        SELECT id as "id!: ClubId" FROM clubs
        WHERE fee_waiver IS NOT NULL AND (?1 IS NULL OR id = ?1)
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    let jury = match rules.as_ref().and_then(|r| r.jury_rules(jury_registration)) {
        Some(_) => jury_facts(conn, club_id).await?,
        None => JuryFacts::default(),
    };
    tx.commit().await?;
    Ok(club_fees(
        &FeeFacts {
            rules,
            riders,
            waived_clubs,
            jury,
        },
        jury_registration,
    ))
}

/// Per rider: the acts they are in (and how many of those are waived), the
/// pair request, and the waivers of the rider and their club.
async fn rider_facts(
    conn: &mut SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<Vec<RiderFeeFacts>> {
    Ok(sqlx::query!(
        r#"
        SELECT
            rider.club_id as "club_id!: ClubId",
            rider.fee_waiver IS NOT NULL as "waived!: bool",
            (SELECT fee_waiver FROM clubs WHERE clubs.id = rider.club_id) IS NOT NULL
                as "club_waived!: bool",
            (SELECT COUNT(*) FROM act_participants WHERE act_participants.rider_id = rider.id)
                as "participations!: i64",
            EXISTS (SELECT 1 FROM pair_requests WHERE pair_requests.rider_id = rider.id)
                as "pair_request!: bool",
            (
                SELECT COUNT(*) FROM act_participants
                JOIN acts ON acts.id = act_participants.act_id
                WHERE act_participants.rider_id = rider.id AND acts.fee_waiver IS NOT NULL
            ) as "waived_participations!: i64"
        FROM rider
        WHERE ?1 IS NULL OR rider.club_id = ?1
        "#,
        club_id
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| RiderFeeFacts {
        club_id: row.club_id,
        waived: row.waived,
        club_waived: row.club_waived,
        participations: row.participations,
        pair_request: row.pair_request,
        waived_participations: row.waived_participations,
    })
    .collect())
}

/// The clubs (all, or `club_id`), all acts and the judges of those clubs with
/// their unit registrations.
async fn jury_facts(
    conn: &mut SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<JuryFacts> {
    let clubs = sqlx::query!(
        r#"
        SELECT id as "id!: ClubId", jury_deployments_override
        FROM clubs
        WHERE ?1 IS NULL OR id = ?1
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryClubFacts {
        club_id: row.id,
        deployments_override: row.jury_deployments_override,
    })
    .collect();

    let mut participants: HashMap<_, Vec<JuryParticipantFacts>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            p.act_id as "act_id!",
            s.club_id as "club_id!: ClubId",
            p.is_replacement as "is_replacement!: bool"
        FROM act_participants p JOIN rider s ON s.id = p.rider_id
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        participants
            .entry(row.act_id)
            .or_default()
            .push(JuryParticipantFacts {
                club_id: row.club_id,
                is_replacement: row.is_replacement,
            });
    }
    let acts = sqlx::query!(
        r#"
        SELECT
            id as "id!",
            withdrawn_at IS NOT NULL as "withdrawn!: bool",
            fee_waiver IS NOT NULL as "fee_waived!: bool",
            category IS NOT NULL as "has_category!: bool",
            act_type as "act_type!: ActType",
            regular_count as "regular_count!: i64",
            owner_club_id as "owner_club: ClubId"
        FROM view_act
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryActFacts {
        withdrawn: row.withdrawn,
        fee_waived: row.fee_waived,
        has_category: row.has_category,
        act_type: row.act_type,
        regular_count: row.regular_count,
        owner_club: row.owner_club,
        participants: participants.remove(&row.id).unwrap_or_default(),
    })
    .collect();

    let mut registrations = judge_registrations(&mut *conn, club_id).await?;
    let judges = sqlx::query!(
        r#"
        SELECT id as "id!: JudgeId", club_id as "club_id!: ClubId", is_trained, max_deployments
        FROM judge
        WHERE ?1 IS NULL OR club_id = ?1
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JudgeFacts {
        club_id: row.club_id,
        is_trained: row.is_trained,
        max_deployments: row.max_deployments,
        registrations: registrations.remove(&row.id).unwrap_or_default(),
    })
    .collect();
    Ok(JuryFacts {
        clubs,
        acts,
        judges,
    })
}

/// The unit registrations of the judges of `club_id` (all clubs without),
/// with what each unit counts for.
pub(crate) async fn judge_registrations(
    conn: &mut SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<HashMap<JudgeId, Vec<UnitRegistration>>> {
    let mut registrations: HashMap<JudgeId, Vec<UnitRegistration>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            r.judge_id as "judge_id!: JudgeId",
            r.unit_id as "unit_id!: JuryUnitId",
            r.task as "task!: JudgeTask",
            u.deployments as "deployments!: i64"
        FROM judge_registration r
        JOIN view_jury_unit u ON u.id = r.unit_id
        JOIN judge ON judge.id = r.judge_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        "#,
        club_id
    )
    .fetch_all(conn)
    .await?
    {
        registrations
            .entry(row.judge_id)
            .or_default()
            .push(UnitRegistration {
                unit_id: row.unit_id,
                task: row.task,
                deployments: row.deployments,
            });
    }
    Ok(registrations)
}
