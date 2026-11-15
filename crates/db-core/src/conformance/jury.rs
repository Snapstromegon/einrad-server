//! The jury area through the trait: units, judges, registrations and the
//! Juryplan. The rules and messages have unit tests in `crate::jury`; these
//! check that a backend loads the facts they need and applies the outcome.

use time::macros::datetime;

use super::{
    World, add_judge, admin, club_judges, club_judges_as, jury_units,
    structure::{category, fields, heat_id, new_final, new_heat},
    timeplan::create_competition_with,
};
use crate::{
    Db,
    act_type::ActType,
    commands::jury::{
        ApplyJuryRemap, AssignJudge, EditJuryUnit, MoveJudgeRegistrations,
        ResolveUnmappedRegistration, SetJuryPlanPublished, UnassignJudge,
    },
    competition::JuryRules,
    error::CommandError,
    ids::{ClubId, JuryUnitId},
    jury::{
        JudgeFields, JudgeRegistration, JudgeTask, JuryUnitFields, JuryUnitKind,
        NOT_REGISTRABLE_MESSAGE, RemapProblem, RemapRegistration,
    },
    queries::jury::{JuryPlan, ListJuryPlan},
};

fn rejected<T: std::fmt::Debug>(result: Result<T, CommandError>) -> String {
    match result {
        Err(CommandError::Rejected(message)) => message,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// A competition with jury rules whose judge registration ended at
/// `registration_end`.
async fn jury_competition(db: &dyn Db, registration_end: Option<time::OffsetDateTime>) {
    create_competition_with(db, |c| {
        c.jury = Some(JuryRules {
            min_age: 16,
            deployments_per_two_acts: 4,
            missing_deployment_fee: 25.0,
            registration_end,
        });
    })
    .await;
}

/// The heats "Vorlauf EM" and "Vorlauf EF", and the final "Finale EM" of the
/// first one; returns their jury units (EM, EF, final).
async fn three_units(db: &dyn Db) -> (JuryUnitId, JuryUnitId, JuryUnitId) {
    let em = category(db, "EM", ActType::Single, true).await;
    let ef = category(db, "EF", ActType::Single, false).await;
    db.add_heat(&admin(), new_heat("Vorlauf EM", "EM", vec![em]))
        .await
        .unwrap();
    db.add_heat(&admin(), new_heat("Vorlauf EF", "EF", vec![ef]))
        .await
        .unwrap();
    let heat = heat_id(db, "Vorlauf EM").await;
    db.add_final(
        &admin(),
        new_final("Finale EM", "FEM", fields(true, &[(heat, 1)])),
    )
    .await
    .unwrap();
    let units = jury_units(db).await;
    let find = |kind, name: &str| {
        units
            .iter()
            .find(|u| u.kind == kind && u.name == name)
            .unwrap()
            .id
    };
    (
        find(JuryUnitKind::Heat, "Vorlauf EM"),
        find(JuryUnitKind::Heat, "Vorlauf EF"),
        find(JuryUnitKind::Final, "Finale EM"),
    )
}

fn judge(name: &str, unit: Option<(JuryUnitKind, JuryUnitId)>) -> JudgeFields {
    JudgeFields {
        firstname: name.into(),
        lastname: "Juror".into(),
        birthdate: datetime!(1990-01-01 0:00 UTC),
        is_trained: true,
        max_deployments: None,
        note: None,
        registrations: unit
            .into_iter()
            .map(|(unit_kind, unit_id)| JudgeRegistration {
                unit_kind,
                unit_id,
                task: JudgeTask::Technical,
            })
            .collect(),
    }
}

async fn plan(
    db: &dyn Db,
    actor: &crate::actor::Actor,
    club_id: Option<ClubId>,
) -> Result<JuryPlan, CommandError> {
    db.list_jury_plan(actor, ListJuryPlan { club_id }).await
}

/// The jury units (with discipline, final and, without a timeplan, no day),
/// their settings, moving registrations, remap files and the judge lists.
pub async fn jury_units_registrations_and_remaps(db: &dyn Db) {
    let w = World::seed(db).await;
    jury_competition(db, None).await;
    let (em, ef, finale) = three_units(db).await;

    let units = jury_units(db).await;
    assert_eq!(
        units.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
        ["Vorlauf EF", "Vorlauf EM", "Finale EM"],
        "units the timeplan doesn't run: heats first, by name, finals last"
    );
    let heat_em = units.iter().find(|u| u.id == em).unwrap();
    assert_eq!(heat_em.act_type, Some(ActType::Single));
    assert_eq!(heat_em.final_id.map(|f| f.0), Some(finale.0));
    assert!(units.iter().all(|u| u.day.is_none() && u.registrable));

    // Jury settings: switching a unit off and the numbers' checks.
    let settings = |id, registrable, deployments| EditJuryUnit {
        fields: JuryUnitFields {
            kind: JuryUnitKind::Heat,
            id,
            registrable,
            deployments,
            needed_technical: 5,
            needed_performance: 5,
            needed_dismounts: 3,
        },
    };
    db.edit_jury_unit(&admin(), settings(ef, false, 3))
        .await
        .unwrap();
    let ef_unit = jury_units(db)
        .await
        .into_iter()
        .find(|u| u.id == ef)
        .unwrap();
    assert!(!ef_unit.registrable && ef_unit.deployments == 3);
    assert_eq!(
        rejected(db.edit_jury_unit(&admin(), settings(ef, true, -1)).await),
        "Einsätze und benötigte JurorInnen dürfen nicht negativ sein."
    );
    assert!(matches!(
        db.edit_jury_unit(&admin(), settings(JuryUnitId::new(), true, 1))
            .await,
        Err(CommandError::NotFound)
    ));

    // Moving a judge's registrations.
    let tina = add_judge(
        db,
        &w.owner_a,
        w.club_a,
        judge("Tina", Some((JuryUnitKind::Heat, em))),
    )
    .await
    .unwrap();
    let move_to = |to_kind, to_id, from_id| MoveJudgeRegistrations {
        judge_id: tina,
        from_id,
        to_kind,
        to_id,
    };
    assert_eq!(
        rejected(
            db.move_judge_registrations(&admin(), move_to(JuryUnitKind::Heat, ef, em))
                .await
        ),
        NOT_REGISTRABLE_MESSAGE
    );
    assert!(matches!(
        db.move_judge_registrations(&admin(), move_to(JuryUnitKind::Heat, JuryUnitId::new(), em))
            .await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        db.move_judge_registrations(&admin(), move_to(JuryUnitKind::Final, finale, ef))
            .await,
        Err(CommandError::NotFound)
    ));
    db.move_judge_registrations(&admin(), move_to(JuryUnitKind::Final, finale, em))
        .await
        .unwrap();
    let listed = club_judges(db, w.club_a).await;
    assert_eq!(listed[0].registrations.len(), 1);
    assert_eq!(listed[0].registrations[0].unit_id, finale);
    assert!(!listed[0].registrations[0].flagged);
    // The unit the jury lead switches off flags what is registered there.
    db.edit_jury_unit(
        &admin(),
        EditJuryUnit {
            fields: JuryUnitFields {
                kind: JuryUnitKind::Final,
                id: finale,
                registrable: false,
                deployments: 1,
                needed_technical: 5,
                needed_performance: 5,
                needed_dismounts: 3,
            },
        },
    )
    .await
    .unwrap();
    assert!(club_judges(db, w.club_a).await[0].registrations[0].flagged);

    // Unmapped registrations only come from a migration: none to resolve.
    assert!(matches!(
        db.resolve_unmapped_registration(
            &admin(),
            ResolveUnmappedRegistration { id: 1, unit: None }
        )
        .await,
        Err(CommandError::NotFound)
    ));

    // A remap file: filled in and found, unfilled, unknown judge, unknown and
    // switched-off units.
    let club = club_judges(db, w.club_a).await[0].club_name.clone();
    let entry = |firstname: &str, jury_unit: &str| RemapRegistration {
        club: club.clone(),
        firstname: firstname.into(),
        lastname: "Juror".into(),
        task: JudgeTask::Performance,
        old_jury_unit: "Alt".into(),
        jury_unit: jury_unit.into(),
    };
    let outcome = db
        .apply_jury_remap(
            &admin(),
            ApplyJuryRemap {
                registrations: vec![
                    entry("Tina", "heat: Vorlauf EM"),
                    entry("Tina", ""),
                    entry("Nobody", "heat: Vorlauf EM"),
                    entry("Tina", "heat: Gibt es nicht"),
                    entry("Tina", "heat: Vorlauf EF"),
                ],
            },
        )
        .await
        .unwrap();
    assert_eq!((outcome.registered, outcome.unfilled.len()), (1, 1));
    assert_eq!(
        outcome
            .not_found
            .iter()
            .map(|(_, problem)| *problem)
            .collect::<Vec<_>>(),
        [
            RemapProblem::JudgeNotFound,
            RemapProblem::JuryUnitNotFound,
            RemapProblem::JuryUnitNotRegistrable
        ]
    );
    assert_eq!(club_judges(db, w.club_a).await[0].registrations.len(), 2);

    // The judge lists: a club's own (club access) and everyone's.
    add_judge(db, &w.owner_b, w.club_b, judge("Ulla", None))
        .await
        .unwrap();
    assert_eq!(db.list_judges(&admin()).await.unwrap().len(), 2);
    assert_eq!(
        club_judges_as(db, &w.owner_b, w.club_b)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        club_judges_as(db, &w.owner_b, w.club_a).await,
        Err(CommandError::Forbidden)
    ));
}

/// Assignments, who sees the Juryplan and when it can be published.
pub async fn jury_plan_assignments_and_publishing(db: &dyn Db) {
    let w = World::seed(db).await;
    // The judge registration ends in 2100: not publishable yet.
    jury_competition(db, None).await;
    let (em, ..) = three_units(db).await;
    let tina = add_judge(
        db,
        &w.owner_a,
        w.club_a,
        judge("Tina", Some((JuryUnitKind::Heat, em))),
    )
    .await
    .unwrap();
    let assign = AssignJudge {
        judge_id: tina,
        unit_kind: JuryUnitKind::Heat,
        unit_id: em,
        task: JudgeTask::Technical,
    };
    db.assign_judge(&admin(), assign.clone()).await.unwrap();
    assert_eq!(
        rejected(db.assign_judge(&admin(), assign).await),
        "Die JurorIn ist hier schon eingeteilt."
    );

    // The jury lead sees all of it while unpublished; a club sees nothing
    // yet and may not ask for everyone.
    let whole = plan(db, &admin(), None).await.unwrap();
    assert!(whole.published_at.is_none());
    assert_eq!(whole.assignments.len(), 1);
    assert_eq!(whole.assignments[0].judge_id, tina);
    let own = plan(db, &w.owner_a, Some(w.club_a)).await.unwrap();
    assert!(own.published_at.is_none() && own.assignments.is_empty());
    assert!(matches!(
        plan(db, &w.owner_a, None).await,
        Err(CommandError::Forbidden)
    ));
    assert!(matches!(
        plan(db, &w.owner_b, Some(w.club_a)).await,
        Err(CommandError::Forbidden)
    ));
    assert!(matches!(
        plan(db, &admin(), Some(ClubId::new())).await,
        Err(CommandError::NotFound)
    ));

    let now = datetime!(2027-04-01 12:00 UTC);
    let publish = |published| SetJuryPlanPublished { published, now };
    assert!(
        rejected(db.set_jury_plan_published(&admin(), publish(true)).await).contains("Juryplan")
    );
    db.set_jury_plan_published(&admin(), publish(false))
        .await
        .unwrap();

    db.unassign_judge(
        &admin(),
        UnassignJudge {
            judge_id: tina,
            unit_id: em,
        },
    )
    .await
    .unwrap();
    assert!(
        plan(db, &admin(), None)
            .await
            .unwrap()
            .assignments
            .is_empty()
    );
    // Removing nothing is fine.
    db.unassign_judge(
        &admin(),
        UnassignJudge {
            judge_id: tina,
            unit_id: em,
        },
    )
    .await
    .unwrap();
}

/// A published Juryplan shows each club its own judges' assignments.
pub async fn a_published_jury_plan_is_visible_to_the_club(db: &dyn Db) {
    let w = World::seed(db).await;
    jury_competition(db, Some(datetime!(2027-03-01 0:00 UTC))).await;
    let (em, ..) = three_units(db).await;
    let tina = add_judge(
        db,
        &w.owner_a,
        w.club_a,
        judge("Tina", Some((JuryUnitKind::Heat, em))),
    )
    .await
    .unwrap();
    db.assign_judge(
        &admin(),
        AssignJudge {
            judge_id: tina,
            unit_kind: JuryUnitKind::Heat,
            unit_id: em,
            task: JudgeTask::Technical,
        },
    )
    .await
    .unwrap();
    let now = datetime!(2027-04-01 12:00 UTC);
    db.set_jury_plan_published(
        &admin(),
        SetJuryPlanPublished {
            published: true,
            now,
        },
    )
    .await
    .unwrap();
    let own = plan(db, &w.owner_a, Some(w.club_a)).await.unwrap();
    assert_eq!(own.published_at, Some(now));
    assert_eq!(own.assignments.len(), 1);
    let other = plan(db, &w.owner_b, Some(w.club_b)).await.unwrap();
    assert!(other.assignments.is_empty());
    db.set_jury_plan_published(
        &admin(),
        SetJuryPlanPublished {
            published: false,
            now,
        },
    )
    .await
    .unwrap();
    assert!(
        plan(db, &w.owner_a, Some(w.club_a))
            .await
            .unwrap()
            .published_at
            .is_none()
    );
}
