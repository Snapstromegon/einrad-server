//! Categories, heats and finals through the trait. The unit tests of
//! `crate::structure_rules` cover every rule and message; these check that a
//! backend loads the facts they need and applies the outcome.

use time::macros::datetime;

use super::{
    World, add_judge, admin, edit_judge, jury_units,
    timeplan::{add_entry, create_competition_with},
};
use crate::{
    Db,
    act_type::ActType,
    clubs::RegistrationError,
    commands::{
        category::{
            AddCategory, CategoryFields, DeleteCategory, EditCategory, MoveCategoryDown,
            MoveCategoryUp, MoveOutcome,
        },
        club::{AddRider, SetSingle},
        finals::{AddFinal, DeleteFinal, EditFinal, SetFinalPlacement, SetFinalPublished},
        heat::{AddHeat, DeleteHeat, EditHeat, SetHeatSkip},
        timeplan::EditTimeplanEntry,
    },
    finals::{FinalFields, SlotGroupInput, SlotPlacement},
    gender::Gender,
    ids::{CategoryId, FinalId, HeatId},
    jury::{JudgeFields, JudgeRegistration, JudgeTask, JuryUnitKind},
    queries::timeplan::PredictTimeplan,
    timeplan::{Audience, EntryFields},
};

fn message<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> String {
    match result {
        Err(
            RegistrationError::Rejected(message)
            | RegistrationError::Conflict(message)
            | RegistrationError::NotFoundMessage(message),
        ) => message,
        other => panic!("expected a message, got {other:?}"),
    }
}

fn is_conflict<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> bool {
    matches!(result, Err(RegistrationError::Conflict(_)))
}

pub(super) async fn category(db: &dyn Db, name: &str, act_type: ActType, male: bool) -> CategoryId {
    db.add_category(
        &admin(),
        AddCategory {
            fields: CategoryFields {
                name: name.into(),
                description: None,
                min_age: None,
                under_age: None,
                act_type,
                is_single_male: male,
                einfahrzeit_seconds: 0,
                act_duration_seconds: 120,
                judge_duration_seconds: 60,
            },
        },
    )
    .await
    .unwrap()
    .0
}

pub(super) fn new_heat(name: &str, code: &str, categories: Vec<CategoryId>) -> AddHeat {
    AddHeat {
        name: name.into(),
        code: code.into(),
        description: None,
        category_ids: categories,
        threshold: None,
    }
}

fn heat_change(
    id: HeatId,
    name: &str,
    code: Option<&str>,
    categories: Vec<CategoryId>,
) -> EditHeat {
    EditHeat {
        heat_id: id,
        new_name: name.into(),
        new_code: code.map(str::to_string),
        description: None,
        category_ids: categories,
        threshold: None,
    }
}

pub(super) fn fields(male: bool, groups: &[(HeatId, i64)]) -> FinalFields {
    FinalFields {
        description: None,
        act_type: ActType::Single,
        is_single_male: male,
        einfahrzeit_seconds: 0,
        act_duration_seconds: 120,
        judge_duration_seconds: 60,
        slot_groups: groups
            .iter()
            .map(|(heat_id, count)| SlotGroupInput {
                heat_id: *heat_id,
                count: *count,
            })
            .collect(),
    }
}

pub(super) fn new_final(name: &str, code: &str, fields: FinalFields) -> AddFinal {
    AddFinal {
        name: name.into(),
        code: code.into(),
        fields,
    }
}

pub(super) async fn heat_id(db: &dyn Db, name: &str) -> HeatId {
    db.list_heats()
        .await
        .unwrap()
        .into_iter()
        .find(|heat| heat.name == name)
        .unwrap()
        .id
}

pub(super) async fn final_id(db: &dyn Db, name: &str) -> FinalId {
    db.list_finals(Some(&admin()))
        .await
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
        .id
}

/// Places acts in a final's slots through the trait.
async fn place_acts(
    db: &dyn Db,
    final_id: FinalId,
    changes: &[SlotPlacement],
) -> Result<(), RegistrationError> {
    db.set_final_placement(
        &admin(),
        SetFinalPlacement {
            final_id,
            changes: changes.to_vec(),
        },
    )
    .await
}

/// Names and codes of heats and finals are unique (a heat or final can keep
/// its own); a heat's categories must exist.
pub async fn structure_names_and_codes_are_unique(db: &dyn Db) {
    create_competition_with(db, |_| {}).await;
    let em = category(db, "EM", ActType::Single, true).await;
    let ef = category(db, "EF", ActType::Single, false).await;
    db.add_heat(&admin(), new_heat("Vorlauf EM", "EM", vec![em]))
        .await
        .unwrap();
    db.add_heat(&admin(), new_heat("Vorlauf EF", "EF", vec![ef]))
        .await
        .unwrap();
    assert_eq!(
        message(
            db.add_heat(&admin(), new_heat("Vorlauf EM", "X", vec![]))
                .await
        ),
        "Eine Vorlaufklasse „Vorlauf EM“ gibt es schon."
    );
    assert_eq!(
        message(db.add_heat(&admin(), new_heat("Neu", "EF", vec![])).await),
        "Eine Vorlaufklasse mit dem Kürzel „EF“ gibt es schon."
    );
    let unknown = CategoryId::new();
    assert!(matches!(
        db.add_heat(&admin(), new_heat("Neu", "N", vec![unknown])).await,
        Err(RegistrationError::NotFoundMessage(m)) if m == format!("Die Kategorie {unknown} gibt es nicht (mehr).")
    ));
    // Nothing of the refused heats stayed.
    assert_eq!(db.list_heats().await.unwrap().len(), 2);

    let heat_em = heat_id(db, "Vorlauf EM").await;
    // Keeping its own name and code is fine; taking another's is not.
    db.edit_heat(&admin(), heat_change(heat_em, "Vorlauf EM", None, vec![em]))
        .await
        .unwrap();
    assert!(is_conflict(
        db.edit_heat(&admin(), heat_change(heat_em, "Vorlauf EF", None, vec![em]))
            .await
    ));
    assert!(is_conflict(
        db.edit_heat(
            &admin(),
            heat_change(heat_em, "Vorlauf EM", Some("EF"), vec![em])
        )
        .await
    ));
    assert!(matches!(
        db.edit_heat(
            &admin(),
            heat_change(HeatId::new(), "Nirgends", None, vec![])
        )
        .await,
        Err(RegistrationError::NotFound)
    ));

    db.add_final(
        &admin(),
        new_final("Finale EM", "FEM", fields(true, &[(heat_em, 3)])),
    )
    .await
    .unwrap();
    assert_eq!(
        message(
            db.add_final(&admin(), new_final("Finale EM", "X", fields(true, &[])))
                .await
        ),
        "Ein Finale „Finale EM“ gibt es schon."
    );
    assert_eq!(
        message(
            db.add_final(&admin(), new_final("Anderes", "FEM", fields(true, &[])))
                .await
        ),
        "Ein Finale mit dem Kürzel „FEM“ gibt es schon."
    );
    let finale = final_id(db, "Finale EM").await;
    db.edit_final(
        &admin(),
        EditFinal {
            final_id: finale,
            new_name: "Finale EM".into(),
            new_code: None,
            fields: fields(true, &[(heat_em, 3)]),
        },
    )
    .await
    .unwrap();
}

/// Heat categories share one discipline, a heat fills one final of its
/// discipline, a category in a heat keeps its discipline, and a heat in a
/// final can't be deleted.
pub async fn heats_and_finals_share_a_discipline(db: &dyn Db) {
    create_competition_with(db, |_| {}).await;
    let em = category(db, "EM", ActType::Single, true).await;
    let ef = category(db, "EF", ActType::Single, false).await;
    let pair = category(db, "P", ActType::GroupSmall, false).await;
    // Categories of other disciplines don't form a heat; a category is in
    // one heat only.
    assert!(
        message(
            db.add_heat(&admin(), new_heat("Gemischt", "G", vec![em, ef]))
                .await
        )
        .contains("EF hat einen anderen Typ")
    );
    db.add_heat(&admin(), new_heat("Vorlauf EM", "EM", vec![em]))
        .await
        .unwrap();
    assert!(
        message(
            db.add_heat(&admin(), new_heat("Nochmal", "N", vec![em]))
                .await
        )
        .contains("gehört schon zur Vorlaufklasse „Vorlauf EM“")
    );
    // A group heat starts skipped.
    db.add_heat(&admin(), new_heat("Vorlauf P", "P", vec![pair]))
        .await
        .unwrap();
    let heat_p = heat_id(db, "Vorlauf P").await;
    let heats = db.list_heats().await.unwrap();
    assert!(!heats.iter().find(|h| h.name == "Vorlauf EM").unwrap().skip);
    assert!(heats.iter().find(|h| h.name == "Vorlauf P").unwrap().skip);
    db.add_heat(&admin(), new_heat("Vorlauf EF", "EF", vec![ef]))
        .await
        .unwrap();
    let heat_em = heat_id(db, "Vorlauf EM").await;
    let heat_ef = heat_id(db, "Vorlauf EF").await;

    // A final's heats must exist and share its discipline.
    assert_eq!(
        message(
            db.add_final(
                &admin(),
                new_final("F", "F", fields(true, &[(HeatId::new(), 1)]))
            )
            .await
        ),
        "Die Vorlaufklasse gibt es nicht."
    );
    assert!(
        message(
            db.add_final(&admin(), new_final("F", "F", fields(true, &[(heat_ef, 1)])))
                .await
        )
        .contains("„Vorlauf EF“ hat einen anderen Typ oder ein anderes Geschlecht")
    );
    assert!(
        message(
            db.add_final(&admin(), new_final("F", "F", fields(true, &[(heat_p, 1)])))
                .await
        )
        .contains("„Vorlauf P“ hat einen anderen Typ")
    );
    db.add_final(
        &admin(),
        new_final("Finale EM", "FEM", fields(true, &[(heat_em, 2)])),
    )
    .await
    .unwrap();
    // A heat fills at most one final.
    assert!(
        message(
            db.add_final(
                &admin(),
                new_final("Zweit", "Z", fields(true, &[(heat_em, 1)]))
            )
            .await
        )
        .contains("füllt schon Finalplätze im Finale „Finale EM“")
    );
    // An empty heat fits any final.
    db.add_heat(&admin(), new_heat("Leer", "L", vec![]))
        .await
        .unwrap();
    let empty = heat_id(db, "Leer").await;
    db.add_final(
        &admin(),
        new_final("Finale Leer", "FL", fields(false, &[(empty, 1)])),
    )
    .await
    .unwrap();

    // The heat can't take categories of another discipline while it fills
    // the final, nor can the category change its own.
    assert!(
        message(
            db.edit_heat(&admin(), heat_change(heat_em, "Vorlauf EM", None, vec![ef]))
                .await
        )
        .contains("gehört schon zur Vorlaufklasse „Vorlauf EF“")
    );
    assert!(
        message(
            db.edit_heat(&admin(), heat_change(empty, "Leer", None, vec![pair]))
                .await
        )
        .contains("gehört schon zur Vorlaufklasse")
    );
    assert!(
        message(
            db.edit_category(
                &admin(),
                EditCategory {
                    category_id: em,
                    fields: CategoryFields {
                        name: "EM".into(),
                        description: None,
                        min_age: None,
                        under_age: None,
                        act_type: ActType::Single,
                        is_single_male: false,
                        einfahrzeit_seconds: 0,
                        act_duration_seconds: 120,
                        judge_duration_seconds: 60,
                    },
                }
            )
            .await
        )
        .contains("EM gehört zur Vorlaufklasse „Vorlauf EM“: Typ und Geschlecht bleiben")
    );
    // A heat in a final can't be deleted; after leaving it, it can.
    assert!(
        message(
            db.delete_heat(&admin(), DeleteHeat { heat_id: heat_em })
                .await
        )
        .contains("füllt Finalplätze in dem Finale „Finale EM“")
    );
    let finale = final_id(db, "Finale EM").await;
    db.delete_final(&admin(), DeleteFinal { final_id: finale })
        .await
        .unwrap();
    db.delete_heat(&admin(), DeleteHeat { heat_id: heat_em })
        .await
        .unwrap();
}

/// Judges registered for a heat or final keep it from being deleted.
pub async fn units_with_judges_are_not_deletable(db: &dyn Db) {
    let w = World::seed(db).await;
    create_competition_with(db, |_| {}).await;
    let em = category(db, "EM", ActType::Single, true).await;
    db.add_heat(&admin(), new_heat("Vorlauf EM", "EM", vec![em]))
        .await
        .unwrap();
    let heat = heat_id(db, "Vorlauf EM").await;
    db.add_final(
        &admin(),
        new_final("Finale EM", "FEM", fields(true, &[(heat, 1)])),
    )
    .await
    .unwrap();
    let finale = final_id(db, "Finale EM").await;
    let units = jury_units(db).await;
    let register = |kind, unit_id| JudgeFields {
        firstname: "Tina".into(),
        lastname: "Juror".into(),
        birthdate: datetime!(1990-01-01 0:00 UTC),
        is_trained: true,
        max_deployments: None,
        note: None,
        registrations: vec![JudgeRegistration {
            unit_kind: kind,
            unit_id,
            task: JudgeTask::Technical,
        }],
    };
    let heat_unit = units
        .iter()
        .find(|u| u.kind == JuryUnitKind::Heat)
        .unwrap()
        .id;
    let final_unit = units
        .iter()
        .find(|u| u.kind == JuryUnitKind::Final)
        .unwrap()
        .id;
    let judge = add_judge(
        db,
        &w.owner_a,
        w.club_a,
        register(JuryUnitKind::Final, final_unit),
    )
    .await
    .unwrap();
    assert!(is_conflict(
        db.delete_final(&admin(), DeleteFinal { final_id: finale })
            .await
    ));
    let mut free = register(JuryUnitKind::Final, final_unit);
    free.registrations.clear();
    edit_judge(db, &w.owner_a, judge, free).await.unwrap();
    db.delete_final(&admin(), DeleteFinal { final_id: finale })
        .await
        .unwrap();

    // The heat has no final any more; a judge on it blocks the delete.
    let judge = add_judge(
        db,
        &w.owner_a,
        w.club_a,
        JudgeFields {
            firstname: "Ulla".into(),
            ..register(JuryUnitKind::Heat, heat_unit)
        },
    )
    .await
    .unwrap();
    assert!(
        message(db.delete_heat(&admin(), DeleteHeat { heat_id: heat }).await)
            .contains("JurorInnen gemeldet oder eingeteilt")
    );
    edit_judge(
        db,
        &w.owner_a,
        judge,
        JudgeFields {
            firstname: "Ulla".into(),
            registrations: vec![],
            ..register(JuryUnitKind::Heat, heat_unit)
        },
    )
    .await
    .unwrap();
    db.delete_heat(&admin(), DeleteHeat { heat_id: heat })
        .await
        .unwrap();
}

/// A judge's name is unique per club (ignoring case and spaces).
pub async fn judge_names_are_unique_per_club(db: &dyn Db) {
    let w = World::seed(db).await;
    let judge = |firstname: &str| JudgeFields {
        firstname: firstname.into(),
        lastname: "Juror".into(),
        birthdate: datetime!(1990-01-01 0:00 UTC),
        is_trained: true,
        max_deployments: None,
        note: None,
        registrations: vec![],
    };
    let tina = add_judge(db, &w.owner_a, w.club_a, judge("Tina"))
        .await
        .unwrap();
    assert_eq!(
        message(add_judge(db, &w.owner_a, w.club_a, judge(" tina ")).await),
        "Tina Juror ist bereits gemeldet."
    );
    // Another club may have the same name; the judge may keep their own.
    add_judge(db, &w.owner_b, w.club_b, judge("Tina"))
        .await
        .unwrap();
    edit_judge(db, &w.owner_a, tina, judge("Tina"))
        .await
        .unwrap();
    let ulla = add_judge(db, &w.owner_a, w.club_a, judge("Ulla"))
        .await
        .unwrap();
    assert!(
        message(edit_judge(db, &w.owner_a, ulla, judge("TINA")).await)
            .contains("Tina Juror ist bereits gemeldet.")
    );
}

/// Skipping a Vorlauf, planning a final once, placing acts in its slots and
/// what a started final locks.
pub async fn final_placement_skip_and_start_rules(db: &dyn Db) {
    let w = World::seed(db).await;
    create_competition_with(db, |_| {}).await;
    let ef = category(db, "EF", ActType::Single, false).await;
    let other = category(db, "EF2", ActType::Single, false).await;
    for (firstname, lastname) in [("Anna", "Muster"), ("Berta", "Muster")] {
        let rider = db
            .add_rider(
                &w.owner_a,
                AddRider {
                    club_id: w.club_a,
                    firstname: firstname.into(),
                    lastname: lastname.into(),
                    birthdate: datetime!(2012-05-01 0:00 UTC),
                    gender: Gender::Female,
                },
            )
            .await
            .unwrap();
        db.set_single(
            &w.owner_a,
            SetSingle {
                rider_id: rider,
                single: true,
            },
        )
        .await
        .unwrap();
    }
    db.add_heat(&admin(), new_heat("Vorlauf EF", "EF", vec![ef, other]))
        .await
        .unwrap();
    let heat = heat_id(db, "Vorlauf EF").await;
    db.add_final(
        &admin(),
        new_final("Finale EF", "FEF", fields(false, &[(heat, 2)])),
    )
    .await
    .unwrap();
    let finale = final_id(db, "Finale EF").await;

    // The Vorlauf can be skipped and restored while nothing started.
    assert!(
        db.set_heat_skip(
            &admin(),
            SetHeatSkip {
                heat_id: heat,
                skip: true
            }
        )
        .await
        .unwrap()
    );
    assert!(
        !db.set_heat_skip(
            &admin(),
            SetHeatSkip {
                heat_id: heat,
                skip: true
            }
        )
        .await
        .unwrap()
    );
    assert!(
        db.set_heat_skip(
            &admin(),
            SetHeatSkip {
                heat_id: heat,
                skip: false
            }
        )
        .await
        .unwrap()
    );
    assert!(matches!(
        db.set_heat_skip(
            &admin(),
            SetHeatSkip {
                heat_id: HeatId::new(),
                skip: true
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));

    // Placement: only acts of the heat's categories, each in one slot.
    let acts = db.list_acts().await.unwrap();
    let in_category = acts.iter().filter(|a| a.category_id == Some(ef)).count();
    assert_eq!(in_category, 2, "both single acts start in EF");
    let (a, b) = (acts[0].id, acts[1].id);
    let slots = db.list_finals(Some(&admin())).await.unwrap()[0]
        .slots
        .iter()
        .map(|s| s.id)
        .collect::<Vec<_>>();
    let place = |slot_id, act_id| SlotPlacement { slot_id, act_id };
    place_acts(
        db,
        finale,
        &[place(slots[0], Some(a)), place(slots[1], Some(b))],
    )
    .await
    .unwrap();
    assert_eq!(
        message(place_acts(db, finale, &[place(slots[1], Some(a))]).await),
        "Eine Kür kann nur einen Finalplatz eines Finales bekommen."
    );
    // Swapping is fine.
    place_acts(
        db,
        finale,
        &[place(slots[0], Some(b)), place(slots[1], Some(a))],
    )
    .await
    .unwrap();
    assert_eq!(
        message(
            place_acts(
                db,
                finale,
                &[place(slots[0], Some(crate::ids::ActId::new()))]
            )
            .await
        ),
        "Die Kür für „1. Finalist (EF)“ gibt es nicht."
    );
    assert_eq!(
        message(place_acts(db, finale, &[place(crate::ids::FinalSlotId::new(), None)]).await),
        "Diesen Finalplatz gibt es in diesem Finale nicht."
    );
    // An act of a category outside the heat doesn't fit.
    let outside = category(db, "EM", ActType::Single, true).await;
    let male = db
        .add_rider(
            &w.owner_a,
            AddRider {
                club_id: w.club_a,
                firstname: "Max".into(),
                lastname: "Müller".into(),
                birthdate: datetime!(2012-05-01 0:00 UTC),
                gender: Gender::Male,
            },
        )
        .await
        .unwrap();
    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: male,
            single: true,
        },
    )
    .await
    .unwrap();
    let male_act = db
        .list_acts()
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.category_id == Some(outside))
        .unwrap()
        .id;
    assert!(
        message(place_acts(db, finale, &[place(slots[0], Some(male_act))]).await)
            .contains("startet in EM, einer Kategorie, die nicht zur Vorlaufklasse „Vorlauf EF“")
    );

    // A final runs in one block only.
    let block = EntryFields::new(None, None, Some(finale), None, None, None).unwrap();
    add_entry(db, &block, None).await.unwrap();
    assert_eq!(
        message(add_entry(db, &block, None).await),
        "Das Finale „Finale EF“ ist schon im Zeitplan; jedes Finale läuft in einem Block."
    );
    let missing = EntryFields::new(None, None, Some(FinalId::new()), None, None, None).unwrap();
    assert_eq!(
        message(add_entry(db, &missing, None).await),
        "Das Finale gibt es nicht."
    );
    let entry = db
        .predict_timeplan(
            None,
            PredictTimeplan {
                audience: Audience::OrganizerTeam,
            },
        )
        .await
        .unwrap()
        .entries[0]
        .id;
    // Editing its own block is fine.
    db.edit_timeplan_entry(
        &admin(),
        EditTimeplanEntry {
            id: entry,
            fields: block.clone(),
            earliest_start_time: None,
        },
    )
    .await
    .unwrap();

    // Once it started: no skip changes, no new slots, no delete, and the
    // placement stays published.
    db.set_final_published(
        &admin(),
        SetFinalPublished {
            final_id: finale,
            published: true,
        },
    )
    .await
    .unwrap();
    db.timeplan_forward(&admin()).await.unwrap();
    assert!(
        message(
            db.set_heat_skip(
                &admin(),
                SetHeatSkip {
                    heat_id: heat,
                    skip: true
                }
            )
            .await
        )
        .contains("hat schon eine Kür begonnen")
    );
    assert!(
        message(
            db.edit_final(
                &admin(),
                EditFinal {
                    final_id: finale,
                    new_name: "Finale EF".into(),
                    new_code: None,
                    fields: fields(false, &[(heat, 3)]),
                }
            )
            .await
        )
        .contains("Finalplätze bleiben, wie sie sind")
    );
    // Unchanged slot groups are no change.
    db.edit_final(
        &admin(),
        EditFinal {
            final_id: finale,
            new_name: "Finale EF".into(),
            new_code: None,
            fields: fields(false, &[(heat, 2)]),
        },
    )
    .await
    .unwrap();
    assert!(
        message(
            db.delete_final(&admin(), DeleteFinal { final_id: finale })
                .await
        )
        .contains("nicht mehr gelöscht werden")
    );
    assert!(
        message(
            db.set_final_published(
                &admin(),
                SetFinalPublished {
                    final_id: finale,
                    published: false
                }
            )
            .await
        )
        .contains("Platzierung bleibt veröffentlicht")
    );
    db.set_final_published(
        &admin(),
        SetFinalPublished {
            final_id: finale,
            published: true,
        },
    )
    .await
    .unwrap();
}

async fn category_names(db: &dyn Db) -> Vec<String> {
    db.list_categories(None)
        .await
        .unwrap()
        .into_iter()
        .map(|category| category.name)
        .collect()
}

/// Categories move over their neighbours (the ends refuse and change
/// nothing), delete once, and the structure rows show categories, heats,
/// finals and their slot groups.
pub async fn category_order_delete_and_structure_rows(db: &dyn Db) {
    create_competition_with(db, |_| {}).await;
    let a = category(db, "A", ActType::Single, true).await;
    let b = category(db, "B", ActType::Single, true).await;
    let c = category(db, "C", ActType::Single, true).await;
    assert_eq!(category_names(db).await, ["A", "B", "C"]);

    let up = |category_id| MoveCategoryUp { category_id };
    let down = |category_id| MoveCategoryDown { category_id };
    assert_eq!(
        db.move_category_up(&admin(), up(b)).await.unwrap(),
        MoveOutcome::Moved
    );
    assert_eq!(category_names(db).await, ["B", "A", "C"]);
    assert_eq!(
        db.move_category_up(&admin(), up(b)).await.unwrap(),
        MoveOutcome::AtEnd
    );
    assert_eq!(
        db.move_category_down(&admin(), down(c)).await.unwrap(),
        MoveOutcome::AtEnd
    );
    assert_eq!(
        db.move_category_down(&admin(), down(b)).await.unwrap(),
        MoveOutcome::Moved
    );
    assert_eq!(category_names(db).await, ["A", "B", "C"]);
    assert_eq!(
        db.move_category_up(&admin(), up(CategoryId::new()))
            .await
            .unwrap(),
        MoveOutcome::NotFound
    );

    // A heat and a final show up in the structure rows.
    db.add_heat(&admin(), new_heat("Vorlauf B", "B", vec![b]))
        .await
        .unwrap();
    let heat = heat_id(db, "Vorlauf B").await;
    db.add_final(
        &admin(),
        new_final("Finale B", "FB", fields(true, &[(heat, 2)])),
    )
    .await
    .unwrap();
    let rows = db.get_structure(&admin()).await.unwrap();
    assert_eq!(
        rows.categories
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert_eq!(rows.categories[1].heat_id, Some(heat));
    assert_eq!(rows.heats.len(), 1);
    assert_eq!(rows.finals.len(), 1);
    assert_eq!(rows.slot_groups.len(), 1);
    assert_eq!(rows.slot_groups[0].slots, 2);

    // Deleting works once.
    let delete = |category_id| DeleteCategory { category_id };
    assert!(db.delete_category(&admin(), delete(a)).await.unwrap());
    assert!(!db.delete_category(&admin(), delete(a)).await.unwrap());
    assert_eq!(category_names(db).await, ["B", "C"]);
}
