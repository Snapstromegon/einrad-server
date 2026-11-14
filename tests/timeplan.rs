//! Integration tests for editing and running the timeplan.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse, rider_body};
use einrad_server::ids::{ActId, ClubId, RiderId};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Iso8601};

/// Format sent by the admin frontend.
const START: &str = "2026-04-19T09:00:00Z";

async fn list_timeplan(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

fn ids(entries: &[Value]) -> Vec<i64> {
    entries.iter().map(|e| e["id"].as_i64().unwrap()).collect()
}

fn labels(entries: &[Value]) -> Vec<String> {
    entries
        .iter()
        .map(|e| {
            e["category"]
                .as_str()
                .or(e["label"].as_str())
                .unwrap()
                .to_string()
        })
        .collect()
}

/// Replaces the seeded timeplan with the given entries.
async fn replace_timeplan(app: &TestApp, admin: &str, entries: &[Value]) {
    for id in ids(&list_timeplan(app).await) {
        let response = app
            .post(
                "/api/command/delete_timeplan_entry",
                json!({ "id": id }),
                Some(admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK);
    }
    for entry in entries {
        let response = app
            .post(
                "/api/command/add_timeplan_entry",
                entry.clone(),
                Some(admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
}

fn category_entry(category: &str, earliest_start_time: Option<&str>) -> Value {
    json!({
        "earliest_start_time": earliest_start_time,
        "duration_seconds": null,
        "label": null,
        "category": category,
    })
}

fn custom_entry(label: &str, duration_seconds: i64) -> Value {
    json!({
        "earliest_start_time": null,
        "duration_seconds": duration_seconds,
        "label": label,
        "category": null,
    })
}

#[tokio::test]
async fn list_timeplan_returns_seeded_entries_in_order() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;

    let entries = list_timeplan(&app).await;

    assert!(!entries.is_empty());
    assert!(ids(&entries).is_sorted());
    assert_eq!(entries[0]["category"], "EMU11");
    assert!(entries[0]["earliest_start_time"].is_string());
    let lunch = entries
        .iter()
        .find(|e| e["label"] == "Mittagspause")
        .unwrap();
    assert_eq!(lunch["duration_seconds"], 45 * 60);
    assert_eq!(lunch["category"], Value::Null);
    assert!(entries.iter().all(|e| e["started_at"].is_null()));
}

#[tokio::test]
async fn timeplan_commands_require_admin() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let user = app.cookie_for(app.create_user("Alex", false).await);
    let before = list_timeplan(&app).await;
    let mut edit = custom_entry("X", 60);
    edit["id"] = json!(1);

    for (uri, body) in [
        ("/api/command/add_timeplan_entry", custom_entry("X", 60)),
        ("/api/command/edit_timeplan_entry", edit),
        ("/api/command/delete_timeplan_entry", json!({ "id": 1 })),
        ("/api/command/move_timeplan_up", json!({ "id": 2 })),
        ("/api/command/move_timeplan_down", json!({ "id": 1 })),
        ("/api/command/timeplan_forward", json!(null)),
        ("/api/command/timeplan_backward", json!(null)),
        (
            "/api/command/set_block_order",
            json!({ "timeplan_id": 1, "slots": [] }),
        ),
        (
            "/api/command/reset_block_order",
            json!({ "timeplan_id": 1 }),
        ),
    ] {
        let response = app.post(uri, body.clone(), Some(&user)).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{uri}");
        let response = app.post(uri, body, None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{uri}");
    }

    assert_eq!(list_timeplan(&app).await, before);
}

#[tokio::test]
async fn add_edit_and_delete_timeplan_entries() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Pause", 900),
        ],
    )
    .await;

    let entries = list_timeplan(&app).await;
    assert_eq!(labels(&entries), ["EWU13", "Pause"]);
    assert_eq!(
        entries[0]["earliest_start_time"],
        "+002026-04-19T09:00:00.000000000Z"
    );
    assert_eq!(entries[1]["duration_seconds"], 900);

    let mut edit = custom_entry("Mittagessen", 1800);
    edit["id"] = entries[1]["id"].clone();
    let response = app
        .post("/api/command/edit_timeplan_entry", edit, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let entries = list_timeplan(&app).await;
    assert_eq!(entries[1]["label"], "Mittagessen");
    assert_eq!(entries[1]["duration_seconds"], 1800);

    let response = app
        .post(
            "/api/command/delete_timeplan_entry",
            json!({ "id": entries[1]["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(labels(&list_timeplan(&app).await), ["EWU13"]);
}

#[tokio::test]
async fn delete_unknown_timeplan_entry_is_not_found() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/delete_timeplan_entry",
            json!({ "id": 999_999 }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn move_timeplan_entries_up_and_down() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[
            custom_entry("A", 60),
            custom_entry("B", 60),
            custom_entry("C", 60),
        ],
    )
    .await;
    let original_ids = ids(&list_timeplan(&app).await);

    let response = app
        .post(
            "/api/command/move_timeplan_up",
            json!({ "id": original_ids[2] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let entries = list_timeplan(&app).await;
    assert_eq!(labels(&entries), ["A", "C", "B"]);
    // Entries swap places and keep their ids.
    assert_eq!(
        ids(&entries),
        [original_ids[0], original_ids[2], original_ids[1]]
    );

    let response = app
        .post(
            "/api/command/move_timeplan_down",
            json!({ "id": original_ids[0] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(labels(&list_timeplan(&app).await), ["C", "A", "B"]);
}

#[tokio::test]
async fn moving_first_entry_up_or_last_entry_down_is_rejected() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[custom_entry("A", 60), custom_entry("B", 60)],
    )
    .await;
    let entry_ids = ids(&list_timeplan(&app).await);

    let up = app
        .post(
            "/api/command/move_timeplan_up",
            json!({ "id": entry_ids[0] }),
            Some(&admin),
        )
        .await;
    assert_eq!(up.status, StatusCode::BAD_REQUEST);

    let down = app
        .post(
            "/api/command/move_timeplan_down",
            json!({ "id": entry_ids[1] }),
            Some(&admin),
        )
        .await;
    assert_eq!(down.status, StatusCode::BAD_REQUEST);

    assert_eq!(labels(&list_timeplan(&app).await), ["A", "B"]);
}

/// The acts of a predicted start block, without its breaks.
fn block_acts(item: &Value) -> Vec<Value> {
    item["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|slot| slot.get("Act").cloned())
        .collect()
}

/// Returns the status of every timeplan item and of the acts of the first item.
async fn progress(app: &TestApp) -> (Vec<String>, Vec<String>) {
    let response = app.get("/api/query/predict_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = response.body["items"].as_array().unwrap();
    let item_status = items
        .iter()
        .map(|i| i["status"].as_str().unwrap().to_string())
        .collect();
    let act_status = block_acts(&items[0])
        .iter()
        .map(|a| a["status"].as_str().unwrap().to_string())
        .collect();
    (item_status, act_status)
}

#[tokio::test]
async fn timeplan_forward_and_backward_walk_through_acts() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    for (order, name) in [(1, "Anna"), (2, "Berta")] {
        let act_id = app.add_single_act(&owner, club_id, name).await;
        app.post(
            "/api/command/set_act_order",
            json!({ "act_id": act_id, "order": order }),
            Some(&admin),
        )
        .await;
    }
    // A pair act for the second category.
    let carla = app
        .add_rider(&owner, rider_body(club_id, "Carla", false, true))
        .await;
    let mut dora = rider_body(club_id, "Dora", false, true);
    dora["partner_id"] = json!(carla);
    app.add_rider(&owner, dora).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("PU13", None),
        ],
    )
    .await;

    let forward = async || {
        let response = app
            .post("/api/command/timeplan_forward", json!(null), Some(&admin))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    };
    let backward = async || {
        let response = app
            .post("/api/command/timeplan_backward", json!(null), Some(&admin))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    };
    let state = |items: [&str; 2], acts: [&str; 2]| {
        (
            items.map(String::from).to_vec(),
            acts.map(String::from).to_vec(),
        )
    };

    // Each step forward, with the state after it.
    let steps = [
        // Category starts with its warm-up time; no act runs yet.
        state(["Started", "Planned"], ["Planned", "Planned"]),
        state(["Started", "Planned"], ["Started", "Planned"]),
        state(["Started", "Planned"], ["Ended", "Planned"]),
        state(["Started", "Planned"], ["Ended", "Started"]),
        // Ending the last act ends the category.
        state(["Ended", "Planned"], ["Ended", "Ended"]),
        // The next category starts with its warm-up time.
        state(["Ended", "Started"], ["Ended", "Ended"]),
    ];
    let initial = state(["Planned", "Planned"], ["Planned", "Planned"]);
    assert_eq!(progress(&app).await, initial);
    for expected in &steps {
        forward().await;
        assert_eq!(&progress(&app).await, expected);
    }

    // Going backwards undoes one step at a time, back to the initial state.
    // Reopening a category also reopens its last act.
    for expected in steps.iter().rev().skip(1) {
        backward().await;
        assert_eq!(&progress(&app).await, expected);
    }
    backward().await;
    assert_eq!(progress(&app).await, initial);
}

#[tokio::test]
async fn timeplan_forward_starts_custom_entry() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(&app, &admin, &[custom_entry("Pause", 900)]).await;

    let response = app
        .post("/api/command/timeplan_forward", json!(null), Some(&admin))
        .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(list_timeplan(&app).await[0]["started_at"].is_string());
}

#[tokio::test]
async fn timeplan_backward_without_progress_does_nothing() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let before = list_timeplan(&app).await;

    let response = app
        .post("/api/command/timeplan_backward", json!(null), Some(&admin))
        .await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(list_timeplan(&app).await, before);
}

#[tokio::test]
async fn timeplan_forward_skips_category_without_acts() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Pause", 900),
        ],
    )
    .await;

    // One step skips the empty block and starts the break.
    app.post("/api/command/timeplan_forward", json!(null), Some(&admin))
        .await;

    let entries = list_timeplan(&app).await;
    assert!(entries[0]["started_at"].is_string(), "{entries:?}");
    assert!(entries[0]["ended_at"].is_string(), "{entries:?}");
    assert!(entries[1]["started_at"].is_string(), "{entries:?}");
    assert!(entries[1]["ended_at"].is_null(), "{entries:?}");
}

async fn step(app: &TestApp, admin: &str, command: &str) -> Vec<String> {
    let response = app
        .post(&format!("/api/command/{command}"), json!(null), Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    progress(app).await.0
}

#[tokio::test]
async fn empty_blocks_are_skipped_in_one_step_and_restored() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_single_act(&owner, club_id, "Anna").await;
    let carla = app
        .add_rider(&owner, rider_body(club_id, "Carla", false, true))
        .await;
    let mut dora = rider_body(club_id, "Dora", false, true);
    dora["partner_id"] = json!(carla);
    app.add_rider(&owner, dora).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
            category_entry("EMU15", None),
            category_entry("PU13", None),
        ],
    )
    .await;

    let s = |states: [&str; 4]| states.map(String::from).to_vec();
    // Warm-up, the act, its end (which ends the block).
    step(&app, &admin, "timeplan_forward").await;
    step(&app, &admin, "timeplan_forward").await;
    let ended = step(&app, &admin, "timeplan_forward").await;
    assert_eq!(ended, s(["Ended", "Planned", "Planned", "Planned"]));
    // Both empty blocks are skipped; the pairs start with their warm-up.
    let skipped = step(&app, &admin, "timeplan_forward").await;
    assert_eq!(skipped, s(["Ended", "Ended", "Ended", "Started"]));
    assert_eq!(progress(&app).await.1, ["Ended"]);

    // One step back undoes the start and the skips together.
    let back = step(&app, &admin, "timeplan_backward").await;
    assert_eq!(back, ended);
    // The next one reopens the last block that really ran.
    let reopened = step(&app, &admin, "timeplan_backward").await;
    assert_eq!(reopened, s(["Started", "Planned", "Planned", "Planned"]));
    assert_eq!(progress(&app).await.1, ["Started"]);
}

#[tokio::test]
async fn forward_with_only_empty_blocks_left_changes_nothing() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;

    let states = step(&app, &admin, "timeplan_forward").await;

    assert_eq!(states, ["Planned", "Planned"]);
}

#[tokio::test]
async fn shared_warmup_of_an_empty_category_moves_to_the_next() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_single_act(&owner, club_id, "Anna").await;
    // EWU13 shares the warm-up of the empty EMU13 before it.
    sqlx::query(
        "UPDATE categories SET einfahrzeit_seconds = CASE name WHEN 'EMU13' THEN 600 ELSE 0 END WHERE name IN ('EMU13', 'EWU13')",
    )
    .execute(&app.db)
    .await
    .unwrap();
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EMU13", Some(START)),
            category_entry("EWU13", None),
        ],
    )
    .await;

    let prediction = app.get("/api/query/predict_timeplan", None).await.body;
    let items = prediction["items"].as_array().unwrap();
    let empty = &items[0]["timeplan_entry"]["Acts"];
    let shared = &items[1]["timeplan_entry"]["Acts"];
    assert_eq!(empty["warmup_seconds"], 0);
    assert_eq!(items[0]["planned_duration"], 0);
    assert_eq!(shared["einfahrzeit_seconds"], 0);
    assert_eq!(shared["warmup_seconds"], 600);
    assert_eq!(items[1]["planned_start"], items[0]["planned_start"]);

    // The rider warms up first instead of starting right away.
    step(&app, &admin, "timeplan_forward").await;
    assert_eq!(progress(&app).await.1, Vec::<String>::new());
    let acts = block_acts(&app.get("/api/query/predict_timeplan", None).await.body["items"][1]);
    assert_eq!(acts[0]["status"], "Planned");
}

#[tokio::test]
async fn act_timer_starts_with_the_first_music_start() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let act_id = app.add_single_act(&owner, club_id, "Anna").await;
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;

    let music_started = async || {
        let response = app.get("/api/query/predict_timeplan", None).await;
        block_acts(&response.body["items"][0])[0]["music_started_at"].clone()
    };
    let report = async |cookie: &str| {
        app.post(
            "/api/command/set_act_music_started",
            json!({ "act_id": act_id }),
            Some(cookie),
        )
        .await
        .status
    };

    // During the warm-up, the act doesn't run yet.
    step(&app, &admin, "timeplan_forward").await;
    assert_eq!(report(&admin).await, StatusCode::OK);
    assert!(music_started().await.is_null());

    step(&app, &admin, "timeplan_forward").await;
    assert!(music_started().await.is_null());
    assert_eq!(report(&owner).await, StatusCode::FORBIDDEN);
    assert!(music_started().await.is_null());
    assert_eq!(report(&admin).await, StatusCode::OK);
    assert!(music_started().await.is_string());

    // Only the first start counts.
    sqlx::query("UPDATE acts SET music_started_at = '2026-04-19 09:15:00' WHERE id = ?")
        .bind(act_id)
        .execute(&app.db)
        .await
        .unwrap();
    assert_eq!(report(&admin).await, StatusCode::OK);
    let kept = music_started().await;
    assert!(
        kept.as_str().unwrap().contains("2026-04-19T09:15:00"),
        "{kept}"
    );

    // Undoing the act's start forgets it.
    step(&app, &admin, "timeplan_backward").await;
    assert!(music_started().await.is_null());
    step(&app, &admin, "timeplan_forward").await;
    assert!(music_started().await.is_null());
}

// Re-planning single acts: start blocks with explicit slots.

/// An act of a single rider; `born` decides the age group.
async fn single(
    app: &TestApp,
    owner: &str,
    club_id: ClubId,
    name: &str,
    male: bool,
    born: &str,
) -> ActId {
    let mut body = rider_body(club_id, name, true, false);
    body["single_male"] = json!(male);
    body["single_female"] = json!(!male);
    body["birthdate"] = json!(format!("{born}-05-01T00:00:00.000Z"));
    let rider = app.add_rider(owner, body).await;
    app.act_of(rider).await
}

async fn predicted_items(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/predict_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["items"].as_array().unwrap().clone()
}

/// The slots of a predicted start block: act ids and break labels.
fn slot_names(item: &Value) -> Vec<String> {
    item["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| match (slot.get("Act"), slot.get("Break")) {
            (Some(act), _) => act["id"].as_str().unwrap().to_string(),
            (_, Some(pause)) => format!("Pause {}", pause["label"].as_str().unwrap()),
            _ => panic!("unknown slot {slot}"),
        })
        .collect()
}

/// The status of every slot of a predicted start block.
fn slot_states(item: &Value) -> Vec<String> {
    item["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| {
            slot.get("Act").or(slot.get("Break")).unwrap()["status"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn act(id: ActId) -> Value {
    json!({ "Act": { "act_id": id } })
}

fn new_break(label: &str, duration_seconds: i64) -> Value {
    json!({ "Break": { "id": null, "label": label, "duration_seconds": duration_seconds } })
}

async fn set_order(
    app: &TestApp,
    admin: &str,
    timeplan_id: i64,
    slots: Vec<Value>,
) -> TestResponse {
    app.post(
        "/api/command/set_block_order",
        json!({ "timeplan_id": timeplan_id, "slots": slots }),
        Some(admin),
    )
    .await
}

fn time_of(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Iso8601::DEFAULT).unwrap()
}

/// Seconds between the planned starts of two slots.
fn planned_gap(a: &Value, b: &Value) -> i64 {
    (time_of(&b["planned_start"]) - time_of(&a["planned_start"])).whole_seconds()
}

fn slot(item: &Value, index: usize) -> Value {
    let slot = &item["timeplan_entry"]["Acts"]["slots"][index];
    slot.get("Act").or(slot.get("Break")).unwrap().clone()
}

/// Sets the default order of acts as the acts overview does.
async fn set_act_order(app: &TestApp, admin: &str, acts: &[ActId]) {
    for (order, act_id) in acts.iter().enumerate() {
        let response = app
            .post(
                "/api/command/set_act_order",
                json!({ "act_id": act_id, "order": order + 1 }),
                Some(admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK);
    }
}

async fn step_ok(app: &TestApp, admin: &str, command: &str) {
    let response = app
        .post(&format!("/api/command/{command}"), json!(null), Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

fn rejected(response: &TestResponse, message: &str) {
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body.to_string().contains(message),
        "expected „{message}“ in {:?}",
        response.body
    );
}

#[tokio::test]
async fn acts_of_several_categories_run_in_any_order() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    // A: EWU13 (3 + 4 min), B: EMU17 (3 + 6 min), C: EMU13 (3 + 4 min).
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let b1 = single(&app, &owner, club_id, "Bernd", true, "2011").await;
    let b2 = single(&app, &owner, club_id, "Bodo", true, "2011").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    let c2 = single(&app, &owner, club_id, "Chris", true, "2015").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU17", None),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);

    let order = [a1, b1, c1, b2, c2, a2];
    let response = set_order(&app, &admin, blocks[0], order.map(act).to_vec()).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        order.map(|id| id.to_string()).to_vec()
    );
    let block = &items[0]["timeplan_entry"]["Acts"];
    assert_eq!(block["home_category"], "EWU13");
    assert_eq!(block["manually_ordered"], true);
    assert_eq!(block["categories"], json!(["EWU13", "EMU17", "EMU13"]));
    // The other blocks lost their acts and are skipped.
    assert!(slot_names(&items[1]).is_empty());
    assert!(slot_names(&items[2]).is_empty());
    assert_eq!(items[1]["planned_duration"], 0);

    // Each act takes its own category's act and judging time.
    let acts: Vec<Value> = (0..6).map(|i| slot(&items[0], i)).collect();
    assert_eq!(acts[1]["category"], "EMU17");
    assert_eq!(acts[1]["category_description"], "Einzel Männlich U17");
    assert_eq!(acts[1]["judge_duration_seconds"], 6 * 60);
    let gaps: Vec<i64> = acts.windows(2).map(|w| planned_gap(&w[0], &w[1])).collect();
    assert_eq!(gaps, [7 * 60, 9 * 60, 7 * 60, 9 * 60, 7 * 60]);
    // 10 min warm-up, then 2 × 7 + 2 × 9 + 2 × 7 minutes.
    assert_eq!(items[0]["planned_duration"], (10 + 14 + 18 + 14) * 60);

    // The CSV lists each act with its own category, in the new order.
    let csv = app.get("/api/query/get_startlist_csv", None).await;
    let text = csv.body.as_str().unwrap();
    let rows: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("Wettbewerb"))
        .map(|l| l.split(';').next().unwrap())
        .collect();
    assert_eq!(
        rows,
        [
            "Einzel Weiblich U13",
            "Einzel Männlich U17",
            "Einzel Männlich U13",
            "Einzel Männlich U17",
            "Einzel Männlich U13",
            "Einzel Weiblich U13"
        ]
    );
}

#[tokio::test]
async fn breaks_in_a_block_shift_the_acts_after_them() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;
    let block = ids(&list_timeplan(&app).await)[0];

    let response = set_order(
        &app,
        &admin,
        block,
        vec![act(a1), new_break("Einfahrzeit", 300), act(a2)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        [a1.to_string(), "Pause Einfahrzeit".into(), a2.to_string()]
    );
    let pause = slot(&items[0], 1);
    assert_eq!(pause["duration_seconds"], 300);
    assert_eq!(planned_gap(&slot(&items[0], 0), &pause), 7 * 60);
    assert_eq!(planned_gap(&pause, &slot(&items[0], 2)), 300);
    assert_eq!(items[0]["planned_duration"], (10 + 7 + 5 + 7) * 60);

    // The CSV gets a line for the break.
    let csv = app.get("/api/query/get_startlist_csv", None).await;
    assert!(
        csv.body.as_str().unwrap().contains(" Einfahrzeit\n"),
        "{}",
        csv.body
    );

    // Editing the break keeps its id; leaving it out deletes it.
    let pause_id = pause["id"].as_i64().unwrap();
    let edit = json!({ "Break": { "id": pause_id, "label": "Warm-up", "duration_seconds": 600 } });
    let response = set_order(&app, &admin, block, vec![act(a1), act(a2), edit]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert_eq!(slot(&items[0], 2)["id"], pause_id);
    assert_eq!(slot(&items[0], 2)["duration_seconds"], 600);
    let response = set_order(&app, &admin, block, vec![act(a2), act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        slot_names(&predicted_items(&app).await[0]),
        [a2.to_string(), a1.to_string()]
    );

    // Breaks need a label and a duration.
    let no_duration = set_order(&app, &admin, block, vec![act(a1), new_break("Pause", 0)]).await;
    rejected(&no_duration, "Dauer");
    let no_label = set_order(&app, &admin, block, vec![act(a1), new_break(" ", 60)]).await;
    rejected(&no_label, "Bezeichnung");
}

#[tokio::test]
async fn acts_move_to_the_end_and_behind_a_break() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    set_act_order(&app, &admin, &[a1, a2]).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Mittagspause", 3600),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let before = predicted_items(&app).await;
    assert_eq!(slot_names(&before[0]), [a1.to_string(), a2.to_string()]);

    // To the end of its block.
    let response = set_order(&app, &admin, blocks[0], vec![act(a2), act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        slot_names(&predicted_items(&app).await[0]),
        [a2.to_string(), a1.to_string()]
    );

    // Behind the lunch break: into the next block, one call for the target.
    let response = set_order(&app, &admin, blocks[2], vec![act(c1), act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert_eq!(slot_names(&items[0]), [a2.to_string()]);
    assert_eq!(slot_names(&items[2]), [c1.to_string(), a1.to_string()]);
    let moved = slot(&items[2], 1);
    assert_eq!(moved["category"], "EWU13");
    assert!(time_of(&moved["planned_start"]) >= time_of(&items[1]["planned_end"]));
    // The first block got shorter, so everything after it moved forward.
    assert_eq!(
        (time_of(&items[1]["planned_start"]) - time_of(&before[1]["planned_start"]))
            .whole_seconds(),
        -7 * 60
    );
}

#[tokio::test]
async fn new_blocks_without_category_and_entries_in_between() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    single(&app, &owner, club_id, "Berta", false, "2015").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Siegerehrung", 1800),
        ],
    )
    .await;
    let first = ids(&list_timeplan(&app).await)[0];

    // A late riders' block right after the first block.
    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": null,
                "duration_seconds": null,
                "label": "Nachstarter",
                "category": null,
                "kind": "acts",
                "einfahrzeit_seconds": 120,
                "after_id": first,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let late = response.body["id"].as_i64().unwrap();
    let entries = list_timeplan(&app).await;
    assert_eq!(ids(&entries)[1], late);
    assert_eq!(entries[1]["kind"], "acts");
    assert_eq!(entries[1]["einfahrzeit_seconds"], 120);

    // Empty until acts are moved there.
    let items = predicted_items(&app).await;
    assert_eq!(items[1]["planned_duration"], 0);
    assert_eq!(
        items[1]["timeplan_entry"]["Acts"]["description"],
        "Nachstarter"
    );
    let response = set_order(&app, &admin, late, vec![act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert_eq!(slot_names(&items[1]), [a1.to_string()]);
    assert_eq!(items[1]["timeplan_entry"]["Acts"]["warmup_seconds"], 120);
    assert_eq!(items[1]["planned_duration"], (2 + 7) * 60);

    // Incomplete entries are rejected.
    for body in [
        json!({ "label": "Pause", "duration_seconds": null, "category": null, "earliest_start_time": null }),
        json!({ "label": null, "duration_seconds": null, "category": null, "earliest_start_time": null, "kind": "acts" }),
    ] {
        let response = app
            .post("/api/command/add_timeplan_entry", body, Some(&admin))
            .await;
        assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    // Turning the block into a custom entry sends its act home.
    let response = app
        .post(
            "/api/command/edit_timeplan_entry",
            json!({
                "id": late,
                "earliest_start_time": null,
                "duration_seconds": 600,
                "label": "Umbau",
                "category": null,
                "kind": "custom",
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert!(slot_names(&items[0]).contains(&a1.to_string()));
}

#[tokio::test]
async fn the_running_block_can_be_replanned_after_the_current_act() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let a3 = single(&app, &owner, club_id, "Clara", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    set_act_order(&app, &admin, &[a1, a2, a3]).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);

    // Warm-up, then the first act runs.
    step_ok(&app, &admin, "timeplan_forward").await;
    step_ok(&app, &admin, "timeplan_forward").await;
    let items = predicted_items(&app).await;
    assert_eq!(slot_states(&items[0]), ["Started", "Planned", "Planned"]);

    // The running act stays first; the rest can change.
    let response = set_order(&app, &admin, blocks[0], vec![act(a1), act(a3), act(a2)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let moved_first = set_order(&app, &admin, blocks[0], vec![act(a2), act(a1), act(a3)]).await;
    rejected(&moved_first, "Gestartete");
    let taken_away = set_order(&app, &admin, blocks[1], vec![act(c1), act(a1)]).await;
    rejected(&taken_away, "begonnen");
    // Acts of later blocks can join the running one.
    let response = set_order(
        &app,
        &admin,
        blocks[0],
        vec![act(a1), act(a3), act(c1), act(a2)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // "Weiter" follows the new order at once.
    step_ok(&app, &admin, "timeplan_forward").await;
    step_ok(&app, &admin, "timeplan_forward").await;
    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        [a1, a3, c1, a2].map(|id| id.to_string()).to_vec()
    );
    assert_eq!(
        slot_states(&items[0]),
        ["Ended", "Started", "Planned", "Planned"]
    );

    // Ended blocks can't be changed.
    for _ in 0..5 {
        step_ok(&app, &admin, "timeplan_forward").await;
    }
    assert_eq!(predicted_items(&app).await[0]["status"], "Ended");
    let ended = set_order(
        &app,
        &admin,
        blocks[0],
        vec![act(a1), act(a3), act(c1), act(a2)],
    )
    .await;
    rejected(&ended, "beendet");
}

#[tokio::test]
async fn forward_and_backward_step_through_acts_and_breaks() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let b1 = single(&app, &owner, club_id, "Bernd", true, "2011").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Siegerehrung", 1800),
        ],
    )
    .await;
    let block = ids(&list_timeplan(&app).await)[0];
    let response = set_order(
        &app,
        &admin,
        block,
        vec![act(a1), new_break("Einfahrzeit U17", 300), act(b1)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let state = async || {
        let items = predicted_items(&app).await;
        (
            items[0]["status"].as_str().unwrap().to_string(),
            slot_states(&items[0]),
        )
    };
    let expect =
        |block: &str, slots: [&str; 3]| (block.to_string(), slots.map(String::from).to_vec());
    let initial = expect("Planned", ["Planned", "Planned", "Planned"]);
    let steps = [
        // The block's warm-up.
        expect("Started", ["Planned", "Planned", "Planned"]),
        expect("Started", ["Started", "Planned", "Planned"]),
        // Judging.
        expect("Started", ["Ended", "Planned", "Planned"]),
        // The break runs until "Weiter" …
        expect("Started", ["Ended", "Started", "Planned"]),
        // … which ends it and starts the next act at once.
        expect("Started", ["Ended", "Ended", "Started"]),
        // Ending the last act ends the block.
        expect("Ended", ["Ended", "Ended", "Ended"]),
    ];
    assert_eq!(state().await, initial);
    for expected in &steps {
        step_ok(&app, &admin, "timeplan_forward").await;
        assert_eq!(&state().await, expected);
    }
    for expected in steps.iter().rev().skip(1) {
        step_ok(&app, &admin, "timeplan_backward").await;
        assert_eq!(&state().await, expected);
    }
    step_ok(&app, &admin, "timeplan_backward").await;
    assert_eq!(state().await, initial);
}

#[tokio::test]
async fn a_break_runs_on_after_its_time_until_forward() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;
    let block = ids(&list_timeplan(&app).await)[0];
    let response = set_order(
        &app,
        &admin,
        block,
        vec![new_break("Einfahrzeit", 60), act(a1)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // Without a block warm-up, the first slot (the break) starts with the
    // block.
    sqlx::query("UPDATE categories SET einfahrzeit_seconds = 0 WHERE name = 'EWU13'")
        .execute(&app.db)
        .await
        .unwrap();
    step_ok(&app, &admin, "timeplan_forward").await;
    let items = predicted_items(&app).await;
    assert_eq!(slot_states(&items[0]), ["Started", "Planned"]);

    // An hour later, it still runs, and the act is predicted from now on.
    sqlx::query(
        "UPDATE timeplan_slots SET started_at = datetime('now', '-1 hour') WHERE act_id IS NULL",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let items = predicted_items(&app).await;
    assert_eq!(slot_states(&items[0]), ["Started", "Planned"]);
    let next = slot(&items[0], 1);
    assert!(
        time_of(&next["predicted_start"]) >= OffsetDateTime::now_utc() - time::Duration::seconds(5)
    );

    step_ok(&app, &admin, "timeplan_forward").await;
    assert_eq!(
        slot_states(&predicted_items(&app).await[0]),
        ["Ended", "Started"]
    );
}

#[tokio::test]
async fn late_acts_join_their_home_block_and_deleted_acts_leave() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;
    let block = ids(&list_timeplan(&app).await)[0];
    let response = set_order(&app, &admin, block, vec![act(a2), act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let a3 = single(&app, &owner, club_id, "Clara", false, "2015").await;
    assert_eq!(
        slot_names(&predicted_items(&app).await[0]),
        [a2, a1, a3].map(|id| id.to_string()).to_vec()
    );

    let rider: RiderId =
        sqlx::query_scalar("SELECT rider_id FROM act_participants WHERE act_id = ?")
            .bind(a1)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let response = app
        .post(
            "/api/command/delete_rider",
            json!({ "rider_id": rider }),
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        slot_names(&predicted_items(&app).await[0]),
        [a2, a3].map(|id| id.to_string()).to_vec()
    );
}

#[tokio::test]
async fn reset_returns_a_block_to_category_order() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    set_act_order(&app, &admin, &[a1, a2]).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let response = set_order(
        &app,
        &admin,
        blocks[0],
        vec![act(c1), act(a2), new_break("Pause", 300), act(a1)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let manual = |entries: Vec<Value>| -> Vec<bool> {
        entries
            .iter()
            .map(|e| e["manually_ordered"].as_bool().unwrap())
            .collect()
    };
    assert_eq!(manual(list_timeplan(&app).await), [true, false]);

    let response = app
        .post(
            "/api/command/reset_block_order",
            json!({ "timeplan_id": blocks[0] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert_eq!(slot_names(&items[0]), [a1.to_string(), a2.to_string()]);
    assert_eq!(
        items[0]["timeplan_entry"]["Acts"]["manually_ordered"],
        false
    );
    assert_eq!(slot_names(&items[1]), [c1.to_string()]);
    assert_eq!(manual(list_timeplan(&app).await), [false, false]);
    let breaks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeplan_slots")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(breaks, 0);

    // Started blocks keep their order.
    step_ok(&app, &admin, "timeplan_forward").await;
    let response = app
        .post(
            "/api/command/reset_block_order",
            json!({ "timeplan_id": blocks[0] }),
            Some(&admin),
        )
        .await;
    rejected(&response, "Gestartete");
}

#[tokio::test]
async fn set_block_order_rejects_invalid_orders() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Pause", 900),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let response = set_order(
        &app,
        &admin,
        blocks[0],
        vec![act(a1), new_break("Pause", 60)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let pause = slot(&predicted_items(&app).await[0], 1)["id"].clone();

    let custom = set_order(&app, &admin, blocks[1], vec![act(a1)]).await;
    rejected(&custom, "Startblöcke");
    let unknown = set_order(&app, &admin, blocks[2], vec![act(ActId::new())]).await;
    rejected(&unknown, "Unbekannte Kür");
    let twice = set_order(&app, &admin, blocks[0], vec![act(a1), act(a1)]).await;
    rejected(&twice, "nur einmal");
    let foreign_break = set_order(
        &app,
        &admin,
        blocks[2],
        vec![json!({ "Break": { "id": pause, "label": "Pause", "duration_seconds": 60 } })],
    )
    .await;
    rejected(&foreign_break, "Unbekannte Pause");
    let missing = set_order(&app, &admin, 999_999, vec![]).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // Nothing changed.
    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        [a1.to_string(), "Pause Pause".into()]
    );
}

#[tokio::test]
async fn started_slots_keep_their_planned_order_whatever_their_times() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let a3 = single(&app, &owner, club_id, "Clara", false, "2015").await;
    set_act_order(&app, &admin, &[a1, a2, a3]).await;
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;
    // Warm-up, a1, its end, a2.
    for _ in 0..4 {
        step_ok(&app, &admin, "timeplan_forward").await;
    }
    // A start time from another clock (e.g. imported data) doesn't reorder.
    sqlx::query("UPDATE acts SET started_at = '2099-01-01 10:00:00' WHERE id = ?")
        .bind(a1)
        .execute(&app.db)
        .await
        .unwrap();

    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        [a1, a2, a3].map(|id| id.to_string()).to_vec()
    );
    assert_eq!(slot_states(&items[0]), ["Ended", "Started", "Planned"]);
    step_ok(&app, &admin, "timeplan_forward").await;
    step_ok(&app, &admin, "timeplan_forward").await;
    assert_eq!(
        slot_states(&predicted_items(&app).await[0]),
        ["Ended", "Ended", "Started"]
    );
}

/// Adds a start block without a category after `after_id`; returns its id.
async fn add_late_block(app: &TestApp, admin: &str, label: &str, after_id: i64) -> i64 {
    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": null,
                "duration_seconds": null,
                "label": label,
                "category": null,
                "kind": "acts",
                "after_id": after_id,
            }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["id"].as_i64().unwrap()
}

#[tokio::test]
async fn acts_are_never_sent_back_into_an_ended_block() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    set_act_order(&app, &admin, &[a1, a2]).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            custom_entry("Pause", 900),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let late = add_late_block(&app, &admin, "Nachstarter", blocks[1]).await;
    let response = set_order(&app, &admin, late, vec![act(a2)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // EWU13 runs its only act and ends.
    for _ in 0..3 {
        step_ok(&app, &admin, "timeplan_forward").await;
    }
    assert_eq!(predicted_items(&app).await[0]["status"], "Ended");

    let reset = app
        .post(
            "/api/command/reset_block_order",
            json!({ "timeplan_id": late }),
            Some(&admin),
        )
        .await;
    rejected(&reset, "schon beendet");
    let left_out = set_order(&app, &admin, late, vec![]).await;
    rejected(&left_out, "schon beendet");
    let deleted = app
        .post(
            "/api/command/delete_timeplan_entry",
            json!({ "id": late }),
            Some(&admin),
        )
        .await;
    rejected(&deleted, "schon beendet");
    let mut to_custom = custom_entry("Umbau", 300);
    to_custom["id"] = json!(late);
    let edited = app
        .post("/api/command/edit_timeplan_entry", to_custom, Some(&admin))
        .await;
    rejected(&edited, "schon beendet");
    assert_eq!(
        slot_names(&predicted_items(&app).await[2]),
        [a2.to_string()]
    );

    // Into another planned block, it still runs.
    let response = set_order(&app, &admin, blocks[2], vec![act(c1), act(a2)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn moving_entries_keeps_started_acts_in_their_block() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    set_act_order(&app, &admin, &[a1, a2]).await;
    // Two blocks of the same category, e.g. a category split by hand.
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EWU13", Some(START)),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    step_ok(&app, &admin, "timeplan_forward").await;
    step_ok(&app, &admin, "timeplan_forward").await;

    let response = app
        .post(
            "/api/command/move_timeplan_up",
            json!({ "id": blocks[1] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let items = predicted_items(&app).await;
    assert_eq!(items[1]["id"], blocks[0]);
    assert_eq!(slot_names(&items[1]), [a1.to_string(), a2.to_string()]);
    assert_eq!(slot_states(&items[1]), ["Started", "Planned"]);
    assert!(slot_names(&items[0]).is_empty());
}

#[tokio::test]
async fn withdrawn_acts_keep_their_place_in_a_replanned_block() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let response = set_order(&app, &admin, blocks[0], vec![act(c1), act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let withdraw = async |withdrawn: bool| {
        let response = app
            .post(
                "/api/command/set_act_withdrawn",
                json!({ "act_id": c1, "withdrawn": withdrawn }),
                Some(&admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    };
    withdraw(true).await;
    // The UI only sees and sends the acts with a category.
    let response = set_order(
        &app,
        &admin,
        blocks[0],
        vec![new_break("Einfahrzeit", 300), act(a1)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    withdraw(false).await;

    let items = predicted_items(&app).await;
    assert_eq!(
        slot_names(&items[0]),
        [c1.to_string(), "Pause Einfahrzeit".into(), a1.to_string()]
    );
    assert!(slot_names(&items[1]).is_empty());
}

#[tokio::test]
async fn a_started_break_comes_only_once() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    sqlx::query("UPDATE categories SET einfahrzeit_seconds = 0 WHERE name = 'EWU13'")
        .execute(&app.db)
        .await
        .unwrap();
    replace_timeplan(&app, &admin, &[category_entry("EWU13", Some(START))]).await;
    let block = ids(&list_timeplan(&app).await)[0];
    let response = set_order(
        &app,
        &admin,
        block,
        vec![new_break("Einfahrzeit", 300), act(a1), act(a2)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    step_ok(&app, &admin, "timeplan_forward").await;
    let items = predicted_items(&app).await;
    assert_eq!(slot_states(&items[0]), ["Started", "Planned", "Planned"]);
    let pause = slot(&items[0], 0)["id"].clone();

    let again =
        |label: &str| json!({ "Break": { "id": pause, "label": label, "duration_seconds": 300 } });
    let twice = set_order(
        &app,
        &admin,
        block,
        vec![again("Einfahrzeit"), act(a1), again("Noch mal"), act(a2)],
    )
    .await;
    rejected(&twice, "Unbekannte Pause");
    // Getting longer while it runs is fine.
    let response = set_order(
        &app,
        &admin,
        block,
        vec![
            json!({ "Break": { "id": pause, "label": "Einfahrzeit", "duration_seconds": 600 } }),
            act(a1),
            act(a2),
        ],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        slot(&predicted_items(&app).await[0], 0)["duration_seconds"],
        600
    );
}

#[tokio::test]
async fn backward_after_a_block_lost_its_last_acts_only_reopens_the_block() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let a2 = single(&app, &owner, club_id, "Berta", false, "2015").await;
    set_act_order(&app, &admin, &[a1, a2]).await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    // Warm-up, a1, its end: a1 is judged.
    for _ in 0..3 {
        step_ok(&app, &admin, "timeplan_forward").await;
    }
    let response = set_order(&app, &admin, blocks[1], vec![act(a2)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // Nothing left in EWU13: "Weiter" ends it without ending an act.
    step_ok(&app, &admin, "timeplan_forward").await;
    assert_eq!(predicted_items(&app).await[0]["status"], "Ended");

    step_ok(&app, &admin, "timeplan_backward").await;
    let items = predicted_items(&app).await;
    assert_eq!(items[0]["status"], "Started");
    assert_eq!(slot_states(&items[0]), ["Ended"]);
}

#[tokio::test]
async fn a_block_that_lost_its_acts_skips_its_breaks() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EWU13", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let response = set_order(
        &app,
        &admin,
        blocks[0],
        vec![new_break("Einfahrzeit", 300), act(a1)],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // The block's warm-up runs, then its only act moves on.
    step_ok(&app, &admin, "timeplan_forward").await;
    let response = set_order(&app, &admin, blocks[1], vec![act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    step_ok(&app, &admin, "timeplan_forward").await;
    let items = predicted_items(&app).await;
    assert_eq!(items[0]["status"], "Ended");
    assert_eq!(slot_states(&items[0]), ["Planned"]);
}

#[tokio::test]
async fn breaks_need_a_duration_in_the_database_too() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let block: i64 = sqlx::query_scalar("SELECT id FROM timeplan ORDER BY position LIMIT 1")
        .fetch_one(&app.db)
        .await
        .unwrap();
    for duration in [None, Some(0)] {
        let insert = sqlx::query(
            "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) VALUES (?, 0, 'Pause', ?)",
        )
        .bind(block)
        .bind(duration)
        .execute(&app.db)
        .await;
        assert!(insert.is_err(), "duration {duration:?} was accepted");
    }
}

#[tokio::test]
async fn a_block_without_category_warms_up_only_with_its_own_time() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let a1 = single(&app, &owner, club_id, "Anna", false, "2015").await;
    let c1 = single(&app, &owner, club_id, "Carl", true, "2015").await;
    // EMU15 is empty; its warm-up waits for the next category sharing it.
    sqlx::query(
        "UPDATE categories SET einfahrzeit_seconds = CASE name WHEN 'EMU15' THEN 600 ELSE 0 END WHERE name IN ('EMU15', 'EMU13')",
    )
    .execute(&app.db)
    .await
    .unwrap();
    replace_timeplan(
        &app,
        &admin,
        &[
            category_entry("EMU15", Some(START)),
            category_entry("EMU13", None),
        ],
    )
    .await;
    let blocks = ids(&list_timeplan(&app).await);
    let late = add_late_block(&app, &admin, "Nachstarter", blocks[0]).await;
    let response = set_order(&app, &admin, late, vec![act(a1)]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let items = predicted_items(&app).await;
    assert_eq!(slot_names(&items[1]), [a1.to_string()]);
    assert_eq!(items[1]["timeplan_entry"]["Acts"]["warmup_seconds"], 0);
    // The shared warm-up passes it by.
    assert_eq!(slot_names(&items[2]), [c1.to_string()]);
    assert_eq!(items[2]["timeplan_entry"]["Acts"]["warmup_seconds"], 600);
}

/// The custom entry labelled `label` in the prediction and in the list.
async fn custom_by_label(app: &TestApp, label: &str) -> (Value, Value) {
    let prediction = app.get("/api/query/predict_timeplan", None).await.body;
    let predicted = prediction["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["timeplan_entry"]["Custom"]["label"] == label)
        .unwrap()["timeplan_entry"]["Custom"]
        .clone();
    let list = app.get("/api/query/list_timeplan", None).await.body;
    let listed = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["label"] == label)
        .unwrap()
        .clone();
    (predicted, listed)
}

#[tokio::test]
async fn custom_entries_show_photos_unless_turned_off() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let added = app
        .post(
            "/api/command/add_timeplan_entry",
            custom_entry("Begrüßung", 300),
            Some(&admin),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let (predicted, listed) = custom_by_label(&app, "Begrüßung").await;
    assert_eq!(predicted["show_photos"], true);
    assert_eq!(listed["show_photos"], true);

    let mut clock_only = custom_entry("Begrüßung", 300);
    clock_only["id"] = added.body["id"].clone();
    clock_only["show_photos"] = json!(false);
    let edited = app
        .post("/api/command/edit_timeplan_entry", clock_only, Some(&admin))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let (predicted, listed) = custom_by_label(&app, "Begrüßung").await;
    assert_eq!(predicted["show_photos"], false);
    assert_eq!(listed["show_photos"], false);
}

/// The custom entry labelled `label` among the prediction's items, if shown.
fn predicted_custom<'a>(prediction: &'a Value, label: &str) -> Option<&'a Value> {
    prediction["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["timeplan_entry"]["Custom"]["label"] == label)
}

/// Adds an internal "Jury-Besprechung" of 10 minutes after the first entry.
async fn add_internal_entry(app: &TestApp, admin: &str) {
    let first = app.get("/api/query/list_timeplan", Some(admin)).await.body[0]["id"].clone();
    let mut entry = custom_entry("Jury-Besprechung", 600);
    entry["internal"] = json!(true);
    entry["after_id"] = first;
    let response = app
        .post("/api/command/add_timeplan_entry", entry, Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn internal_entries_are_a_pause_for_everyone_but_the_organizer_team() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    add_internal_entry(&app, &admin).await;

    let full = app
        .get("/api/query/predict_timeplan", Some(&admin))
        .await
        .body;
    let internal = predicted_custom(&full, "Jury-Besprechung").expect("admins see it");
    assert_eq!(internal["timeplan_entry"]["Custom"]["internal"], true);

    // Public: the same entry at the same time, as a plain "Pause".
    let public = app.get("/api/query/predict_timeplan", None).await.body;
    assert!(predicted_custom(&public, "Jury-Besprechung").is_none());
    let pause = public["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == internal["id"])
        .unwrap();
    assert_eq!(pause["timeplan_entry"]["Custom"]["label"], "Pause");
    assert_eq!(pause["timeplan_entry"]["Custom"]["internal"], false);
    assert_eq!(pause["predicted_start"], internal["predicted_start"]);
    assert_eq!(
        public["items"][2]["predicted_start"],
        full["items"][2]["predicted_start"]
    );

    // The info board asks for the public version, whoever is logged in.
    let board = app
        .get("/api/query/predict_timeplan?public=true", Some(&admin))
        .await
        .body;
    assert!(predicted_custom(&board, "Jury-Besprechung").is_none());
    assert!(predicted_custom(&board, "Pause").is_some());

    for (name, role, sees) in [
        ("Moderation", "moderation", true),
        ("Jury", "jury_lead", true),
        ("Musik", "music_control", true),
        ("Desk", "check_in", false),
    ] {
        let cookie = app.cookie_for(app.create_user_with_roles(name, &[role]).await);
        let prediction = app
            .get("/api/query/predict_timeplan", Some(&cookie))
            .await
            .body;
        assert_eq!(
            predicted_custom(&prediction, "Jury-Besprechung").is_some(),
            sees,
            "{role}"
        );
        let list = app
            .get("/api/query/list_timeplan", Some(&cookie))
            .await
            .body;
        let label = &list[1]["label"];
        assert_eq!(
            label,
            if sees { "Jury-Besprechung" } else { "Pause" },
            "{role}"
        );
        assert_eq!(list[1]["internal"], sees, "{role}");
    }

    let csv = app.get("/api/query/get_startlist_csv", Some(&admin)).await;
    let csv = csv.body.as_str().unwrap();
    assert!(!csv.contains("Jury-Besprechung"));
    assert!(csv.contains("Pause"));
}

#[tokio::test]
async fn start_blocks_cant_be_internal() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": null,
                "duration_seconds": null,
                "label": "Nachstarter",
                "category": null,
                "kind": "acts",
                "internal": true,
            }),
            Some(&admin),
        )
        .await;
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
}
