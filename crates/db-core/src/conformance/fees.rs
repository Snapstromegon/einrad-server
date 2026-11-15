//! Fees, fee waivers, the jury obligation and group rules through the
//! trait. The unit tests of `crate::fees`, `crate::jury` and `crate::groups`
//! cover the arithmetic and every edge; these check that a backend loads
//! the facts the pure functions need.

use time::macros::datetime;

use super::{World, create_group, delete_group, set_members, timeplan::create_competition_with};
use crate::{
    Db,
    act_type::ActType,
    bookings,
    categories::CategoryFields,
    clubs::{Actor, RegistrationError},
    commands::{
        admin::SetActWithdrawn,
        club::{AddRider, LinkPair, RequestPair, SetSingle},
        group::GroupMember,
    },
    competition::JuryRules,
    fee_waivers::Target,
    fees::ClubFees,
    gender::Gender,
    heats::NewHeat,
    ids::{ClubId, RiderId},
    jury::{JudgeFields, JudgeRegistration, JudgeTask, JuryAssignment, JuryUnitKind},
};

fn message<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> String {
    match result {
        Err(RegistrationError::Rejected(message)) => message,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

async fn rider(db: &dyn Db, actor: &Actor, club: ClubId, firstname: &str) -> RiderId {
    db.add_rider(
        actor,
        AddRider {
            club_id: club,
            firstname: firstname.into(),
            lastname: "Muster".into(),
            birthdate: datetime!(2012-05-01 0:00 UTC),
            gender: Gender::Female,
        },
    )
    .await
    .unwrap()
}

async fn category(db: &dyn Db) -> crate::ids::CategoryId {
    db.add_category(&CategoryFields {
        name: "EM".into(),
        description: None,
        min_age: None,
        under_age: None,
        act_type: ActType::Single,
        is_single_male: false,
        einfahrzeit_seconds: 0,
        act_duration_seconds: 120,
        judge_duration_seconds: 60,
    })
    .await
    .unwrap()
    .0
}

fn treasurer(w: &World, roles: &[crate::roles::Role]) -> bookings::Actor {
    bookings::Actor::new(
        w.organizer.user_id,
        "Erika Musterfrau",
        roles.to_vec(),
        None,
    )
}

async fn fees(db: &dyn Db, club: ClubId, jury_registration: bool) -> ClubFees {
    db.fees_of_club(jury_registration, club).await.unwrap()
}

/// Start fees: every start counts (single, pair request, groups), at most
/// the cap per rider; waivers of a rider, an act and a club drop starts and
/// show up as what was waived; revoking them charges again. A blank reason
/// is refused.
pub async fn fees_and_amount_due(db: &dyn Db) {
    let w = World::seed(db).await;
    create_competition_with(db, |c| {
        c.fee_per_start = 10.0;
        c.max_fee_per_rider = Some(25.0);
    })
    .await;
    category(db).await;
    // Without riders nobody owes anything.
    assert!(db.fees_by_club(true).await.unwrap().is_empty());

    let anna = rider(db, &w.owner_a, w.club_a, "Anna").await;
    let berta = rider(db, &w.owner_a, w.club_a, "Berta").await;
    let dora = rider(db, &w.owner_a, w.club_a, "Dora").await;
    let carla = rider(db, &w.owner_b, w.club_b, "Carla").await;
    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: anna,
            single: true,
        },
    )
    .await
    .unwrap();
    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: berta,
            single: true,
        },
    )
    .await
    .unwrap();
    db.request_pair(
        &w.owner_a,
        RequestPair {
            rider_id: berta,
            partner_name: None,
        },
    )
    .await
    .unwrap();
    db.set_single(
        &w.owner_b,
        SetSingle {
            rider_id: carla,
            single: true,
        },
    )
    .await
    .unwrap();
    // Dora: a pair request and two groups, three starts, capped.
    db.request_pair(
        &w.owner_a,
        RequestPair {
            rider_id: dora,
            partner_name: None,
        },
    )
    .await
    .unwrap();
    let small = create_group(db, &w.owner_a, w.club_a, ActType::GroupSmall, "Wirbel")
        .await
        .unwrap();
    let large = create_group(db, &w.owner_a, w.club_a, ActType::GroupLarge, "Sturm")
        .await
        .unwrap();
    for group in [small, large] {
        set_members(
            db,
            &w.owner_a,
            w.club_a,
            group,
            &[GroupMember {
                rider_id: dora,
                is_replacement: false,
            }],
        )
        .await
        .unwrap();
    }

    let due = |fees: &ClubFees| {
        let a = fees.amount_due;
        (a.start_fees, a.jury_fee, a.total, a.waived)
    };
    // 10 + 20 + capped 25.
    assert_eq!(due(&fees(db, w.club_a, true).await), (55.0, 0.0, 55.0, 0.0));
    assert_eq!(due(&fees(db, w.club_b, true).await), (10.0, 0.0, 10.0, 0.0));
    // No jury rules: no obligation either way.
    assert!(fees(db, w.club_a, true).await.jury.is_none());
    let all = db.fees_by_club(false).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[&w.club_a], fees(db, w.club_a, false).await);

    let roles = [];
    let actor = treasurer(&w, &roles);
    assert!(
        message(
            db.set_fee_waiver(&actor, Target::Rider(anna), Some("  \t"))
                .await
        )
        .contains("Grund")
    );
    assert!(matches!(
        db.set_fee_waiver(&actor, Target::Rider(RiderId::new()), Some("Härtefall"))
            .await,
        Err(RegistrationError::NotFound)
    ));
    // A waived rider pays nothing.
    db.set_fee_waiver(&actor, Target::Rider(anna), Some("  Härtefall "))
        .await
        .unwrap();
    assert_eq!(
        due(&fees(db, w.club_a, true).await),
        (45.0, 0.0, 45.0, 10.0)
    );
    // A waived act drops its start; the cap now leaves 2 starts = 20.
    db.set_fee_waiver(&actor, Target::Act(large), Some("Sponsor"))
        .await
        .unwrap();
    assert_eq!(
        due(&fees(db, w.club_a, true).await),
        (40.0, 0.0, 40.0, 15.0)
    );
    // A waived club pays nothing at all.
    db.set_fee_waiver(&actor, Target::Club(w.club_b), Some("Gast"))
        .await
        .unwrap();
    assert_eq!(due(&fees(db, w.club_b, true).await), (0.0, 0.0, 0.0, 10.0));
    // Revoking charges again.
    for target in [
        Target::Rider(anna),
        Target::Act(large),
        Target::Club(w.club_b),
    ] {
        db.set_fee_waiver(&actor, target, None).await.unwrap();
    }
    assert_eq!(due(&fees(db, w.club_a, true).await), (55.0, 0.0, 55.0, 0.0));
    assert_eq!(due(&fees(db, w.club_b, true).await), (10.0, 0.0, 10.0, 0.0));
}

/// The jury obligation: started Küren of the club, two per round of
/// deployments; trained judges count their technical and performance
/// registrations once per unit; untrained people count none and only take
/// dismounts; an override credits instead; the fee follows the jury
/// registration and is waived with the club.
pub async fn jury_obligation_counts_acts_and_judges(db: &dyn Db) {
    let w = World::seed(db).await;
    create_competition_with(db, |c| {
        c.fee_per_start = 10.0;
        c.jury = Some(JuryRules {
            min_age: 16,
            deployments_per_two_acts: 4,
            missing_deployment_fee: 25.0,
            registration_end: None,
        });
    })
    .await;
    let category = category(db).await;
    for firstname in ["Anna", "Berta", "Carla"] {
        let id = rider(db, &w.owner_a, w.club_a, firstname).await;
        db.set_single(
            &w.owner_a,
            SetSingle {
                rider_id: id,
                single: true,
            },
        )
        .await
        .unwrap();
    }
    db.add_heat(&NewHeat {
        name: "Vorlauf EM".into(),
        code: "EM".into(),
        description: None,
        category_ids: vec![category],
        threshold: None,
    })
    .await
    .unwrap();
    let heat = db.jury_unit_rows().await.unwrap().units[0].id;
    let registration = |task| JudgeRegistration {
        unit_kind: JuryUnitKind::Heat,
        unit_id: heat,
        task,
    };
    let judge = |name: &str, is_trained, max_deployments, tasks: &[JudgeTask]| JudgeFields {
        firstname: name.into(),
        lastname: "Juror".into(),
        birthdate: datetime!(1990-01-01 0:00 UTC),
        is_trained,
        max_deployments,
        note: None,
        registrations: tasks.iter().copied().map(registration).collect(),
    };
    let trained = db
        .add_judge(
            &w.owner_a,
            w.club_a,
            judge(
                "Tina",
                true,
                None,
                &[JudgeTask::Technical, JudgeTask::Performance],
            ),
        )
        .await
        .unwrap();
    // Untrained people and dismount-only registrations count nothing; a
    // maximum of 1 caps Cleo.
    db.add_judge(
        &w.owner_a,
        w.club_a,
        judge("Ulla", false, None, &[JudgeTask::Dismounts]),
    )
    .await
    .unwrap();
    db.add_judge(
        &w.owner_a,
        w.club_a,
        judge("Dana", true, None, &[JudgeTask::Dismounts]),
    )
    .await
    .unwrap();
    db.add_judge(
        &w.owner_a,
        w.club_a,
        judge("Cleo", true, Some(1), &[JudgeTask::Technical]),
    )
    .await
    .unwrap();

    // 3 acts: two rounds of 4 deployments; the heat counts 2.
    let with = fees(db, w.club_a, true).await;
    let jury = with.jury.unwrap();
    assert_eq!(
        (jury.acts, jury.required, jury.registered, jury.credited),
        (3, 8, 3, 3)
    );
    assert_eq!((jury.missing, jury.fee), (5, 125.0));
    assert_eq!(with.amount_due.jury_fee, 125.0);
    assert_eq!(with.amount_due.total, 30.0 + 125.0);
    // Club B has no acts: an obligation of 0.
    let b = fees(db, w.club_b, true).await.jury.unwrap();
    assert_eq!((b.acts, b.required, b.missing, b.fee), (0, 0, 0, 0.0));
    // Without the jury registration there is no jury fee and no obligation.
    let off = fees(db, w.club_a, false).await;
    assert!(off.jury.is_none());
    assert_eq!(off.amount_due.total, 30.0);

    let judges = db.list_judges(Some(w.club_a)).await.unwrap();
    let counted = |name: &str| {
        let j = judges.iter().find(|j| j.firstname == name).unwrap();
        (j.registered_deployments, j.counted_deployments)
    };
    assert_eq!(counted("Tina"), (2, 2));
    assert_eq!(counted("Ulla"), (2, 0));
    assert_eq!(counted("Dana"), (2, 0));
    assert_eq!(counted("Cleo"), (2, 1));

    // Untrained people only count dismounts.
    let ulla = judges.iter().find(|j| j.firstname == "Ulla").unwrap().id;
    let assign = |judge_id, task| JuryAssignment {
        judge_id,
        unit_kind: JuryUnitKind::Heat,
        unit_id: heat,
        task,
    };
    assert_eq!(
        message(db.assign_judge(&assign(ulla, JudgeTask::Performance)).await),
        "Nicht geschulte Personen können nur Abstiege zählen."
    );
    db.assign_judge(&assign(ulla, JudgeTask::Dismounts))
        .await
        .unwrap();
    db.assign_judge(&assign(trained, JudgeTask::Technical))
        .await
        .unwrap();
    assert_eq!(
        message(
            db.assign_judge(&assign(trained, JudgeTask::Technical))
                .await
        ),
        "Die JurorIn ist hier schon eingeteilt."
    );

    // An admin's credit replaces the registered deployments.
    db.set_jury_deployments_override(w.club_a, Some(8))
        .await
        .unwrap();
    let jury = fees(db, w.club_a, true).await.jury.unwrap();
    assert_eq!(
        (
            jury.credited_override,
            jury.credited,
            jury.missing,
            jury.fee
        ),
        (Some(8), 8, 0, 0.0)
    );
    db.set_jury_deployments_override(w.club_a, Some(0))
        .await
        .unwrap();
    assert_eq!(fees(db, w.club_a, true).await.jury.unwrap().missing, 8);
    db.set_jury_deployments_override(w.club_a, None)
        .await
        .unwrap();

    // A waived club keeps its obligation but owes no jury fee.
    let roles = [];
    db.set_fee_waiver(&treasurer(&w, &roles), Target::Club(w.club_a), Some("Gast"))
        .await
        .unwrap();
    let waived = fees(db, w.club_a, true).await;
    assert_eq!(waived.jury.unwrap().missing, 5);
    assert_eq!(
        (
            waived.amount_due.jury_fee,
            waived.amount_due.total,
            waived.amount_due.waived
        ),
        (0.0, 0.0, 155.0)
    );
}

/// What counts toward the jury obligation of a club, loaded through the
/// trait: a pair across clubs counts for the club with the lowest id; a
/// withdrawn act counts unless its fee is waived; a group counts for its
/// owner with at least one regular rider (also an incomplete group), but not
/// with only replacements, whatever club they come from.
pub async fn jury_obligation_edge_cases(db: &dyn Db) {
    let w = World::seed(db).await;
    create_competition_with(db, |c| {
        c.fee_per_start = 10.0;
        c.jury = Some(JuryRules {
            min_age: 16,
            deployments_per_two_acts: 4,
            missing_deployment_fee: 25.0,
            registration_end: None,
        });
    })
    .await;
    category(db).await;
    db.add_category(&CategoryFields {
        name: "PA".into(),
        description: None,
        min_age: None,
        under_age: None,
        act_type: ActType::Pair,
        is_single_male: false,
        einfahrzeit_seconds: 0,
        act_duration_seconds: 120,
        judge_duration_seconds: 60,
    })
    .await
    .unwrap();

    let anna = rider(db, &w.owner_a, w.club_a, "Anna").await;
    let berta = rider(db, &w.owner_a, w.club_a, "Berta").await;
    let dora = rider(db, &w.owner_a, w.club_a, "Dora").await;
    let gerda = rider(db, &w.owner_a, w.club_a, "Gerda").await;
    let carla = rider(db, &w.owner_b, w.club_b, "Carla").await;
    let erna = rider(db, &w.owner_b, w.club_b, "Erna").await;
    let hanna = rider(db, &w.owner_b, w.club_b, "Hanna").await;

    // A pair across clubs counts once, for the lowest club.
    db.link_pair(
        &w.organizer,
        LinkPair {
            rider_id: anna,
            partner_id: carla,
        },
    )
    .await
    .unwrap();
    let lowest = w.club_a.min(w.club_b);
    // Singles: Erna starts; Berta withdraws; Dora withdraws with a waiver.
    for (owner, id) in [(&w.owner_b, erna), (&w.owner_a, berta), (&w.owner_a, dora)] {
        db.set_single(
            owner,
            SetSingle {
                rider_id: id,
                single: true,
            },
        )
        .await
        .unwrap();
    }
    let act_of = |name: &'static str| async move {
        db.list_acts()
            .await
            .unwrap()
            .into_iter()
            .find(|act| act.participants.iter().any(|p| p.firstname == name))
            .unwrap()
            .id
    };
    let (berta_act, dora_act) = (act_of("Berta").await, act_of("Dora").await);
    db.set_act_withdrawn(
        &w.organizer,
        SetActWithdrawn {
            act_id: berta_act,
            withdrawn: true,
        },
    )
    .await
    .unwrap();
    db.set_act_withdrawn(
        &w.organizer,
        SetActWithdrawn {
            act_id: dora_act,
            withdrawn: true,
        },
    )
    .await
    .unwrap();
    let roles = [];
    db.set_fee_waiver(
        &treasurer(&w, &roles),
        Target::Act(dora_act),
        Some("Abgesagt"),
    )
    .await
    .unwrap();

    // An incomplete group (one regular rider, one replacement of the other
    // club) counts for its owner; a group of replacements only doesn't.
    let member = |rider_id, is_replacement| GroupMember {
        rider_id,
        is_replacement,
    };
    let incomplete = create_group(db, &w.owner_a, w.club_a, ActType::GroupSmall, "Wirbel")
        .await
        .unwrap();
    set_members(
        db,
        &w.owner_a,
        w.club_a,
        incomplete,
        &[member(gerda, false)],
    )
    .await
    .unwrap();
    set_members(db, &w.owner_b, w.club_b, incomplete, &[member(hanna, true)])
        .await
        .unwrap();
    let only_replacements = create_group(db, &w.owner_a, w.club_a, ActType::GroupSmall, "Ersatz")
        .await
        .unwrap();
    set_members(
        db,
        &w.owner_b,
        w.club_b,
        only_replacements,
        &[member(hanna, true)],
    )
    .await
    .unwrap();

    // A: Berta (withdrawn, charged), the incomplete group, and the pair if A
    // is the lowest. B: Erna, and the pair if B is.
    let (acts_a, acts_b) = if lowest == w.club_a { (3, 1) } else { (2, 2) };
    let obligation = |club| async move { fees(db, club, true).await.jury.unwrap() };
    let (jury_a, jury_b) = (obligation(w.club_a).await, obligation(w.club_b).await);
    assert_eq!((jury_a.acts, jury_b.acts), (acts_a, acts_b));
    assert_eq!(
        (jury_a.required, jury_b.required),
        (
            if acts_a > 2 { 8 } else { 4 },
            if acts_b > 2 { 8 } else { 4 }
        )
    );
    // Nobody registered judges: every required deployment is missing.
    assert_eq!(
        (jury_a.missing, jury_a.fee, jury_b.missing, jury_b.fee),
        (
            jury_a.required,
            25.0 * jury_a.required as f64,
            jury_b.required,
            25.0 * jury_b.required as f64
        )
    );
    // Start fees follow the riders, not the counting club: A has Anna, Berta
    // and Gerda (Dora's start is waived); B has Carla, Erna and Hanna's two
    // starts as a replacement.
    let due = |fees: ClubFees| {
        let a = fees.amount_due;
        (a.start_fees, a.jury_fee, a.total, a.waived)
    };
    let (fees_a, fees_b) = (
        fees(db, w.club_a, true).await,
        fees(db, w.club_b, true).await,
    );
    assert_eq!(due(fees_a), (30.0, jury_a.fee, 30.0 + jury_a.fee, 10.0));
    assert_eq!(due(fees_b), (40.0, jury_b.fee, 40.0 + jury_b.fee, 0.0));
    // Waiving the withdrawn Berta's start drops her from the obligation.
    db.set_fee_waiver(
        &treasurer(&w, &roles),
        Target::Act(berta_act),
        Some("Abgesagt"),
    )
    .await
    .unwrap();
    let after = fees(db, w.club_a, true).await.jury.unwrap();
    assert_eq!(after.acts, acts_a - 1);
}

/// Groups: only the owner club deletes one, and not while riders of other
/// clubs are in it (the organizer may); a club sets only its own riders,
/// each once; names are unique ignoring case.
pub async fn group_delete_and_member_rules(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna").await;
    let carla = rider(db, &w.owner_b, w.club_b, "Carla").await;
    let group = create_group(db, &w.owner_a, w.club_a, ActType::GroupSmall, "Wirbel")
        .await
        .unwrap();
    assert_eq!(
        message(create_group(db, &w.owner_b, w.club_b, ActType::GroupLarge, "wirbel").await),
        "Der Gruppenname „wirbel“ ist bereits vergeben."
    );
    let member = |rider_id| GroupMember {
        rider_id,
        is_replacement: false,
    };
    // Own riders only, each once.
    assert!(
        message(set_members(db, &w.owner_a, w.club_a, group, &[member(carla)]).await)
            .contains("eigenen FahrerInnen")
    );
    assert!(
        message(
            set_members(
                db,
                &w.owner_a,
                w.club_a,
                group,
                &[member(anna), member(anna)]
            )
            .await
        )
        .contains("nur einmal")
    );
    assert!(matches!(
        set_members(db, &w.owner_a, w.club_a, group, &[member(RiderId::new())]).await,
        Err(RegistrationError::Rejected(_))
    ));
    set_members(db, &w.owner_a, w.club_a, group, &[member(anna)])
        .await
        .unwrap();
    set_members(db, &w.owner_b, w.club_b, group, &[member(carla)])
        .await
        .unwrap();

    assert!(
        message(delete_group(db, &w.owner_a, group).await)
            .contains("Die Gruppe „Wirbel“ kann nicht gelöscht werden")
    );
    assert!(matches!(
        delete_group(db, &w.owner_b, group).await,
        Err(RegistrationError::Forbidden)
    ));
    // The other club's riders leave, then the owner may delete.
    set_members(db, &w.owner_b, w.club_b, group, &[])
        .await
        .unwrap();
    delete_group(db, &w.owner_a, group).await.unwrap();
    assert!(db.list_groups(&w.organizer).await.unwrap().is_empty());

    // The organizer deletes a group with foreign riders.
    let group = create_group(db, &w.owner_a, w.club_a, ActType::GroupSmall, "Sturm")
        .await
        .unwrap();
    set_members(db, &w.owner_b, w.club_b, group, &[member(carla)])
        .await
        .unwrap();
    delete_group(db, &w.organizer, group).await.unwrap();
    assert!(matches!(
        delete_group(db, &w.organizer, group).await,
        Err(RegistrationError::NotFound)
    ));
}
