//! The live timeplan through the trait: "Weiter" and "Zurück" over blocks,
//! breaks and skipped blocks, and moving entries. The unit tests of
//! `crate::timeplan` cover every state of the step function; these check
//! that a backend loads, applies and undoes them the same way.

use time::macros::{date, datetime};

use super::World;
use crate::{
    Db,
    act_type::ActType,
    branding::Branding,
    categories::CategoryFields,
    clubs::RegistrationError,
    commands::club::{AddRider, SetSingle},
    competition::{Competition, Features},
    gender::Gender,
    ids::{ActId, CategoryId},
    seed::{CompetitionPlan, ImagePlan},
    timeplan::{Audience, BlockSlotInput, Entry, EntryFields, EntryKind, Slot},
};

fn competition() -> Competition {
    Competition {
        id: "conformance-2027".into(),
        name: "Konformität 2027".into(),
        location: "Beistadt".into(),
        start_date: date!(2027 - 05 - 01),
        end_date: date!(2027 - 05 - 02),
        determining_day: date!(2027 - 05 - 01),
        registration_start: datetime!(2000-01-01 0:00 UTC),
        registration_end: datetime!(2100-01-01 0:00 UTC),
        music_upload_end: datetime!(2100-01-01 0:00 UTC),
        fee_per_start: 10.0,
        max_fee_per_rider: None,
        venue: None,
        contact_email: None,
        announcement_url: None,
        photos_url: None,
        payment_deadline: None,
        payment_account_holder: None,
        payment_iban: None,
        payment_bic: None,
        payment_reference: None,
        jury: None,
        features: Features::default(),
        photos: None,
        primary_color: None,
        secondary_color: None,
    }
}

/// A competition without any planning yet.
async fn create_competition(db: &dyn Db) {
    create_competition_with(db, |_| {}).await;
}

/// Like [`create_competition`], with its settings changed by `change`.
pub(super) async fn create_competition_with(db: &dyn Db, change: impl FnOnce(&mut Competition)) {
    let mut competition = competition();
    change(&mut competition);
    let plan = CompetitionPlan {
        competition,
        primary_color: Branding::Keep,
        secondary_color: Branding::Keep,
        logo: ImagePlan::Keep,
        break_image: ImagePlan::Keep,
        sponsors: None,
        categories: vec![],
        heats: vec![],
        finals: vec![],
        timeplan: vec![],
    };
    db.create_competition("conformance-2027", &|_| Ok(plan.clone()))
        .await
        .unwrap();
}

/// One category (every age, no warm-up) with two single acts.
async fn category_with_two_acts(db: &dyn Db, w: &World) -> CategoryId {
    create_competition(db).await;
    let (category, _) = db
        .add_category(&CategoryFields {
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
        .unwrap();
    for firstname in ["Anna", "Berta"] {
        let rider = db
            .add_rider(
                &w.owner_a,
                AddRider {
                    club_id: w.club_a,
                    firstname: firstname.into(),
                    lastname: "Muster".into(),
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
    category
}

fn custom(label: &str) -> EntryFields {
    EntryFields::new(None, None, None, Some(label.into()), Some(60), None).unwrap()
}

/// What has started and ended, by entry and slot, without the times.
fn state(entries: &[Entry]) -> String {
    let flags = |started: bool, ended: bool| match (started, ended) {
        (false, _) => "-",
        (true, false) => "running",
        (true, true) => "done",
    };
    entries
        .iter()
        .map(|entry| {
            let slots: Vec<&str> = entry
                .slots
                .iter()
                .map(|slot| flags(slot.is_started(), slot.is_ended()))
                .collect();
            format!(
                "{}[{}]",
                flags(entry.started_at.is_some(), entry.ended_at.is_some()),
                slots.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn entries(db: &dyn Db) -> Vec<Entry> {
    db.load_timeplan(Audience::OrganizerTeam)
        .await
        .unwrap()
        .entries
}

fn act_ids(entries: &[Entry]) -> Vec<ActId> {
    entries[0]
        .slots
        .iter()
        .filter_map(Slot::act)
        .map(|act| act.id)
        .collect()
}

/// "Weiter" walks a block's acts and breaks, the entries after it and skips
/// an empty block; "Zurück" undoes every step to the same state. Only the end
/// of an act (or finalist) tells the caller to stop the music.
pub async fn timeplan_forward_and_backward(db: &dyn Db) {
    let w = World::seed(db).await;
    let category = category_with_two_acts(db, &w).await;
    let fields = |label: Option<&str>, category| {
        EntryFields::new(
            Some(EntryKind::Acts),
            category,
            None,
            label.map(Into::into),
            None,
            None,
        )
        .unwrap()
    };
    let block = db
        .add_timeplan_entry(&fields(None, Some(category)), None, None)
        .await
        .unwrap();
    db.add_timeplan_entry(&custom("Pause"), None, None)
        .await
        .unwrap();
    db.add_timeplan_entry(&fields(Some("Nachstarter"), None), None, None)
        .await
        .unwrap();
    db.add_timeplan_entry(&custom("Siegerehrung"), None, None)
        .await
        .unwrap();
    let acts = act_ids(&entries(db).await);
    assert_eq!(acts.len(), 2);
    db.set_block_order(
        block,
        &[
            BlockSlotInput::Act { act_id: acts[0] },
            BlockSlotInput::Break {
                id: None,
                label: "Umbau".into(),
                duration_seconds: 60,
            },
            BlockSlotInput::Act { act_id: acts[1] },
        ],
    )
    .await
    .unwrap();

    // Each forward step: the music stops only when an act ended.
    let expected = [
        ("running[running,-,-] -[] -[] -[]", false),
        ("running[done,-,-] -[] -[] -[]", true),
        ("running[done,running,-] -[] -[] -[]", false),
        ("running[done,done,running] -[] -[] -[]", false),
        // Ending the last act ends the block in the same step.
        ("done[done,done,done] -[] -[] -[]", true),
        ("done[done,done,done] running[] -[] -[]", false),
        ("done[done,done,done] done[] -[] -[]", false),
        // The empty "Nachstarter" is skipped on the way.
        ("done[done,done,done] done[] done[] running[]", false),
        ("done[done,done,done] done[] done[] done[]", false),
    ];
    let mut states = vec![state(&entries(db).await)];
    assert_eq!(states[0], "-[-,-,-] -[] -[] -[]");
    for (index, (state_after, ended_act)) in expected.iter().enumerate() {
        let ended = db.timeplan_forward().await.unwrap();
        let now = state(&entries(db).await);
        assert_eq!((now.as_str(), ended), (*state_after, *ended_act), "{index}");
        states.push(now);
    }
    // At the end nothing changes.
    assert!(!db.timeplan_forward().await.unwrap());
    assert_eq!(state(&entries(db).await), *states.last().unwrap());

    // Undoing the first step takes two: the first act goes back to planned,
    // then the block's start is undone.
    for index in (1..states.len()).rev() {
        db.timeplan_backward().await.unwrap();
        let expected = if index == 1 {
            "running[-,-,-] -[] -[] -[]"
        } else {
            &states[index - 1]
        };
        assert_eq!(state(&entries(db).await), expected, "back from {index}");
    }
    db.timeplan_backward().await.unwrap();
    assert_eq!(state(&entries(db).await), states[0]);
    // Back at the start nothing changes either.
    db.timeplan_backward().await.unwrap();
    assert_eq!(state(&entries(db).await), states[0]);
}

/// Entries move up and down one place; the first can't go up, the last not
/// down. A new entry goes after the one named, which has to exist.
pub async fn timeplan_entries_move_and_insert(db: &dyn Db) {
    let ids = |entries: Vec<_>| -> Vec<i64> { entries.into_iter().map(|e: Entry| e.id).collect() };
    let a = db
        .add_timeplan_entry(&custom("A"), None, None)
        .await
        .unwrap();
    let b = db
        .add_timeplan_entry(&custom("B"), None, None)
        .await
        .unwrap();
    let c = db
        .add_timeplan_entry(&custom("C"), None, None)
        .await
        .unwrap();
    assert_eq!(ids(entries(db).await), [a, b, c]);

    assert!(!db.move_timeplan_entry(a, true).await.unwrap());
    assert!(!db.move_timeplan_entry(c, false).await.unwrap());
    assert!(!db.move_timeplan_entry(c + 100, true).await.unwrap());
    assert_eq!(ids(entries(db).await), [a, b, c]);
    assert!(db.move_timeplan_entry(b, true).await.unwrap());
    assert_eq!(ids(entries(db).await), [b, a, c]);
    assert!(db.move_timeplan_entry(b, false).await.unwrap());
    assert!(db.move_timeplan_entry(c, true).await.unwrap());
    assert_eq!(ids(entries(db).await), [a, c, b]);

    let d = db
        .add_timeplan_entry(&custom("D"), None, Some(a))
        .await
        .unwrap();
    assert_eq!(ids(entries(db).await), [a, d, c, b]);
    let missing = db
        .add_timeplan_entry(&custom("E"), None, Some(b + 100))
        .await;
    assert!(matches!(missing, Err(RegistrationError::NotFound)));
    assert_eq!(ids(entries(db).await), [a, d, c, b]);
}

/// A started block keeps its order; an unstarted one can be reset.
pub async fn timeplan_started_block_keeps_its_order(db: &dyn Db) {
    let w = World::seed(db).await;
    let category = category_with_two_acts(db, &w).await;
    let fields = EntryFields::new(None, Some(category), None, None, None, None).unwrap();
    let block = db.add_timeplan_entry(&fields, None, None).await.unwrap();
    db.reset_block_order(block).await.unwrap();
    db.timeplan_forward().await.unwrap();
    let error = db.reset_block_order(block).await.unwrap_err();
    match error {
        RegistrationError::Rejected(message) => {
            assert_eq!(message, "Gestartete Blöcke behalten ihre Reihenfolge.");
        }
        other => panic!("not a message: {other:?}"),
    }
}
