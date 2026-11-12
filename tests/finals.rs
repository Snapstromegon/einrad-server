//! Integration tests for finals: reserved slots in the timeplan.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Iso8601};

/// Format sent by the admin frontend.
const START: &str = "2026-04-19T09:00:00Z";

/// The final in `tests/fixtures/competition.toml`.
const FINAL: &str = "Finale EW";

async fn list_finals(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_finals", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn list_timeplan(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn predicted_items(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/predict_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["items"].as_array().unwrap().clone()
}

/// Replaces the seeded timeplan with the given entries.
async fn replace_timeplan(app: &TestApp, admin: &str, entries: &[Value]) {
    for entry in list_timeplan(app).await {
        let response = app
            .post(
                "/api/command/delete_timeplan_entry",
                json!({ "id": entry["id"] }),
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

fn final_entry(final_name: &str, earliest_start_time: Option<&str>) -> Value {
    json!({
        "earliest_start_time": earliest_start_time,
        "duration_seconds": null,
        "label": null,
        "category": null,
        "final_name": final_name,
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

/// The finalist slots of a predicted block.
fn finalists(item: &Value) -> Vec<Value> {
    item["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| slot["Finalist"].clone())
        .collect()
}

fn texts(values: &[Value], key: &str) -> Vec<String> {
    values
        .iter()
        .map(|value| value[key].as_str().unwrap().to_string())
        .collect()
}

fn time_of(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Iso8601::DEFAULT).unwrap()
}

async fn step(app: &TestApp, admin: &str, command: &str) {
    let response = app
        .post(&format!("/api/command/{command}"), json!(null), Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// The block's status and its finalists' statuses, after each step.
async fn progress(app: &TestApp) -> (String, Vec<String>) {
    let item = predicted_items(app).await.remove(0);
    (
        item["status"].as_str().unwrap().to_string(),
        texts(&finalists(&item), "status"),
    )
}

fn rejected(response: &TestResponse, message: &str) {
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body.to_string().contains(message),
        "expected „{message}“ in {:?}",
        response.body
    );
}

fn final_body(name: &str, slot_groups: Value) -> Value {
    json!({
        "name": name,
        "description": "Finale Einzel Männlich",
        "act_type": "single",
        "is_single_male": true,
        "einfahrzeit_seconds": 600,
        "act_duration_seconds": 180,
        "judge_duration_seconds": 240,
        "slot_groups": slot_groups,
    })
}

#[tokio::test]
async fn finals_are_seeded_with_their_slots() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let mut finals = list_finals(&app).await;
    let slots = finals[0]["slots"].take();
    assert_eq!(
        texts(slots.as_array().unwrap(), "label"),
        [
            "1. Finalist (JE EW)",
            "2. Finalist (JE EW)",
            "1. Finalist (E EW)"
        ]
    );
    assert!(
        slots
            .as_array()
            .unwrap()
            .iter()
            .all(|slot| slot["act_id"].is_null())
    );
    assert_eq!(
        finals,
        [json!({
            "name": FINAL,
            "description": "Finale Einzel Weiblich",
            "act_type": "single",
            "is_single_male": false,
            "order": 1,
            "einfahrzeit_seconds": 300,
            "act_duration_seconds": 180,
            "judge_duration_seconds": 120,
            "slot_groups": [
                { "heat": "JE EW", "count": 2 },
                { "heat": "E EW", "count": 1 },
            ],
            // From its heats.
            "categories": ["EWU11", "EWU13", "EWU15", "EWU17"],
            "slots": null,
            "placement_published": false,
            "started": false,
            "timeplan_id": null,
        })]
    );
}

#[tokio::test]
async fn a_final_block_reserves_its_slots_with_the_finals_durations() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(
        &app,
        &admin,
        &[
            custom_entry("Begrüßung", 600),
            final_entry(FINAL, None),
            custom_entry("Siegerehrung", 1800),
        ],
    )
    .await;
    // The first entry needs the earliest start.
    let first = list_timeplan(&app).await[0]["id"].clone();
    let response = app
        .post(
            "/api/command/edit_timeplan_entry",
            json!({
                "id": first,
                "earliest_start_time": START,
                "duration_seconds": 600,
                "label": "Begrüßung",
                "category": null,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let entries = list_timeplan(&app).await;
    assert_eq!(entries[1]["kind"], "acts");
    assert_eq!(entries[1]["final_name"], FINAL);
    assert_eq!(entries[1]["category"], Value::Null);
    assert_eq!(list_finals(&app).await[0]["timeplan_id"], entries[1]["id"]);

    let items = predicted_items(&app).await;
    let block = &items[1]["timeplan_entry"]["Acts"];
    assert_eq!(block["final_name"], FINAL);
    assert_eq!(block["home_category"], Value::Null);
    assert_eq!(block["description"], "Finale Einzel Weiblich");
    assert_eq!(block["categories"], json!([]));
    assert_eq!(block["einfahrzeit_seconds"], 300);
    assert_eq!(block["warmup_seconds"], 300);
    let slots = finalists(&items[1]);
    assert_eq!(
        texts(&slots, "label"),
        [
            "1. Finalist (JE EW)",
            "2. Finalist (JE EW)",
            "1. Finalist (E EW)"
        ]
    );
    assert_eq!(texts(&slots, "group_label"), ["JE EW", "JE EW", "E EW"]);
    for slot in &slots {
        assert_eq!(slot["final_name"], FINAL);
        assert_eq!(slot["final_description"], "Finale Einzel Weiblich");
        assert_eq!(slot["act_duration_seconds"], 180);
        assert_eq!(slot["judge_duration_seconds"], 120);
        assert_eq!(slot["status"], "Planned");
    }

    // Warm-up, then each slot with its act and judging time.
    let block_start = time_of(&items[1]["planned_start"]);
    assert_eq!(
        (time_of(&slots[0]["planned_start"]) - block_start).whole_seconds(),
        300
    );
    assert_eq!(
        (time_of(&slots[1]["planned_start"]) - time_of(&slots[0]["planned_start"])).whole_seconds(),
        300
    );
    assert_eq!(items[1]["planned_duration"], 300 + 3 * 300);
    assert_eq!(items[2]["planned_start"], items[1]["planned_end"]);

    // The finalists aren't known: the startlist CSV lists the slots as
    // comments, without start numbers.
    let response = app.get("/api/query/get_startlist_csv", None).await;
    assert_eq!(response.status, StatusCode::OK);
    let csv = response.body.as_str().unwrap();
    for label in [
        "Finale Einzel Weiblich",
        "1. Finalist (JE EW)",
        "1. Finalist (E EW)",
    ] {
        assert!(
            csv.lines()
                .any(|line| line.starts_with("# ") && line.ends_with(label)),
            "{label} in {csv}"
        );
    }
    assert!(!csv.contains(";1. Finalist"), "{csv}");
}

#[tokio::test]
async fn finalists_run_and_are_judged_like_acts() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(&app, &admin, &[final_entry(FINAL, Some(START))]).await;

    let planned = ["Planned", "Planned", "Planned"];
    assert_eq!(
        progress(&app).await,
        ("Planned".into(), planned.map(Into::into).to_vec())
    );

    let expected = [
        // The warm-up.
        ("Started", ["Planned", "Planned", "Planned"]),
        ("Started", ["Started", "Planned", "Planned"]),
        // Judging.
        ("Started", ["Ended", "Planned", "Planned"]),
        ("Started", ["Ended", "Started", "Planned"]),
        ("Started", ["Ended", "Ended", "Planned"]),
        ("Started", ["Ended", "Ended", "Started"]),
        // The last one ends the block.
        ("Ended", ["Ended", "Ended", "Ended"]),
    ];
    for (status, slots) in expected {
        step(&app, &admin, "timeplan_forward").await;
        assert_eq!(
            progress(&app).await,
            (status.to_string(), slots.map(Into::into).to_vec())
        );
    }
    assert!(list_finals(&app).await[0]["started"].as_bool().unwrap());

    // And back again, step by step.
    for (status, slots) in expected.iter().rev().skip(1) {
        step(&app, &admin, "timeplan_backward").await;
        assert_eq!(
            progress(&app).await,
            (status.to_string(), slots.map(Into::into).to_vec())
        );
    }
    step(&app, &admin, "timeplan_backward").await;
    assert_eq!(
        progress(&app).await,
        ("Planned".into(), planned.map(Into::into).to_vec())
    );
    assert!(!list_finals(&app).await[0]["started"].as_bool().unwrap());
}

#[tokio::test]
async fn a_final_runs_in_one_block_with_its_slots() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    replace_timeplan(&app, &admin, &[final_entry(FINAL, Some(START))]).await;
    let id = list_timeplan(&app).await[0]["id"].clone();

    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            final_entry(FINAL, None),
            Some(&admin),
        )
        .await;
    rejected(&response, "schon im Zeitplan");
    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            final_entry("Finale XY", None),
            Some(&admin),
        )
        .await;
    rejected(&response, "gibt es nicht");
    let mut both = final_entry(FINAL, None);
    both["category"] = json!("EWU13");
    let response = app
        .post("/api/command/add_timeplan_entry", both, Some(&admin))
        .await;
    rejected(&response, "nicht beides");

    // Its slots can be re-ordered (#344), but never leave it.
    let response = app
        .post(
            "/api/command/set_block_order",
            json!({ "timeplan_id": id, "slots": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(finalists(&predicted_items(&app).await[0]).len(), 3);
    let response = app
        .post(
            "/api/command/reset_block_order",
            json!({ "timeplan_id": id }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // Editing the block keeps the final (and doesn't count as a second one).
    let mut entry = final_entry(FINAL, Some(START));
    entry["id"] = id.clone();
    entry["einfahrzeit_seconds"] = json!(0);
    let response = app
        .post("/api/command/edit_timeplan_entry", entry, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let item = predicted_items(&app).await.remove(0);
    assert_eq!(item["timeplan_entry"]["Acts"]["warmup_seconds"], 0);
    assert_eq!(finalists(&item).len(), 3);
}

/// Adds a heat through the admin API.
async fn add_heat(app: &TestApp, admin: &str, name: &str, categories: &[&str]) {
    let response = app
        .post(
            "/api/command/add_heat",
            json!({ "name": name, "description": null, "categories": categories }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn admins_add_edit_and_delete_finals() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    add_heat(&app, &admin, "E EM 1", &["EMU17", "EMU19", "EMU21"]).await;
    add_heat(&app, &admin, "E EM 2", &["EMU23", "EMU25"]).await;
    add_heat(&app, &admin, "E P 2", &["PU23", "PU25"]).await;
    let groups = json!([{ "heat": " E EM 1 ", "count": 2 }, { "heat": "E EM 2", "count": 1 }]);

    for cookie in [None, Some(owner.as_str())] {
        let response = app
            .post(
                "/api/command/add_final",
                final_body("Finale EM", groups.clone()),
                cookie,
            )
            .await;
        assert_ne!(response.status, StatusCode::OK);
    }
    let mut group_final = final_body("Finale KG", json!([{ "heat": "E EM 1", "count": 1 }]));
    group_final["act_type"] = json!("group_small");
    for (body, message) in [
        (final_body(" ", groups.clone()), "Namen"),
        (final_body(FINAL, groups.clone()), "gibt es schon"),
        (
            final_body("Finale EM", json!([])),
            "mindestens einen Finalplatz",
        ),
        (
            final_body("Finale EM", json!([{ "heat": "E EM 1", "count": 0 }])),
            "mindestens einen Platz",
        ),
        (
            final_body("Finale EM", json!([{ "heat": "", "count": 1 }])),
            "Vorlauf",
        ),
        (
            final_body("Finale EM", json!([{ "heat": "E EM 9", "count": 1 }])),
            "E EM 9",
        ),
        // Every heat competes in the final's discipline.
        (
            final_body("Finale EM", json!([{ "heat": "E P 2", "count": 1 }])),
            "E P 2",
        ),
        // Group categories are their own final.
        (group_final, "Einzel- und Paarküren"),
    ] {
        let response = app.post("/api/command/add_final", body, Some(&admin)).await;
        rejected(&response, message);
    }

    let response = app
        .post(
            "/api/command/add_final",
            final_body(" Finale  EM ", groups),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let finals = list_finals(&app).await;
    assert_eq!(finals.len(), 2);
    assert_eq!(finals[1]["name"], "Finale EM");
    assert_eq!(finals[1]["order"], 2);
    assert_eq!(finals[1]["is_single_male"], true);
    assert_eq!(
        finals[1]["slot_groups"],
        json!([{ "heat": "E EM 1", "count": 2 }, { "heat": "E EM 2", "count": 1 }])
    );
    assert_eq!(
        finals[1]["categories"],
        json!(["EMU17", "EMU19", "EMU21", "EMU23", "EMU25"])
    );

    // Planned, renamed: the timeplan follows. Pairs are never male.
    replace_timeplan(&app, &admin, &[final_entry("Finale EM", Some(START))]).await;
    let mut body = final_body("Finale Paar", json!([{ "heat": "E P 2", "count": 4 }]));
    body["name"] = json!("Finale EM");
    body["new_name"] = json!("Finale Paar");
    body["act_type"] = json!("pair");
    let response = app
        .post("/api/command/edit_final", body, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let finals = list_finals(&app).await;
    assert_eq!(finals[1]["name"], "Finale Paar");
    assert_eq!(finals[1]["is_single_male"], false);
    assert_eq!(
        finals[1]["slot_groups"],
        json!([{ "heat": "E P 2", "count": 4 }])
    );
    assert_eq!(list_timeplan(&app).await[0]["final_name"], "Finale Paar");
    let item = predicted_items(&app).await.remove(0);
    assert_eq!(
        texts(&finalists(&item), "label"),
        [
            "1. Finalist (E P 2)",
            "2. Finalist (E P 2)",
            "3. Finalist (E P 2)",
            "4. Finalist (E P 2)"
        ]
    );

    // Once it has started, its slots stay; other fields can still change.
    step(&app, &admin, "timeplan_forward").await;
    step(&app, &admin, "timeplan_forward").await;
    let mut body = final_body("Finale Paar", json!([{ "heat": "E P 2", "count": 3 }]));
    body["new_name"] = json!("Finale Paar");
    body["act_type"] = json!("pair");
    let response = app
        .post("/api/command/edit_final", body.clone(), Some(&admin))
        .await;
    rejected(&response, "schon begonnen");
    body["slot_groups"] = json!([{ "heat": "E P 2", "count": 4 }]);
    body["judge_duration_seconds"] = json!(60);
    let response = app
        .post("/api/command/edit_final", body, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let item = predicted_items(&app).await.remove(0);
    let slots = finalists(&item);
    assert_eq!(slots[0]["status"], "Started");
    assert_eq!(slots[0]["judge_duration_seconds"], 60);
    let response = app
        .post(
            "/api/command/delete_final",
            json!({ "name": "Finale Paar" }),
            Some(&admin),
        )
        .await;
    rejected(&response, "schon begonnen");

    // A final that hasn't started is deleted with its timeplan block.
    step(&app, &admin, "timeplan_backward").await;
    step(&app, &admin, "timeplan_backward").await;
    let response = app
        .post(
            "/api/command/delete_final",
            json!({ "name": "Finale Paar" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(texts(&list_finals(&app).await, "name"), [FINAL]);
    assert!(list_timeplan(&app).await.is_empty());
    let response = app
        .post(
            "/api/command/delete_final",
            json!({ "name": "Finale Paar" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

/// The ids of the final's slots, in start order.
async fn slot_ids(app: &TestApp) -> Vec<Value> {
    list_finals(app).await[0]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| slot["id"].clone())
        .collect()
}

/// The acts placed in the final's slots, as `who` sees them.
async fn placed(app: &TestApp, who: Option<&str>) -> Vec<Value> {
    let response = app.get("/api/query/list_finals", who).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body[0]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| slot["act_id"].clone())
        .collect()
}

async fn place(app: &TestApp, admin: &str, slots: Value) -> TestResponse {
    app.post(
        "/api/command/set_final_placement",
        json!({ "name": FINAL, "slots": slots }),
        Some(admin),
    )
    .await
}

async fn publish(app: &TestApp, admin: &str, published: bool) -> TestResponse {
    app.post(
        "/api/command/set_final_published",
        json!({ "name": FINAL, "published": published }),
        Some(admin),
    )
    .await
}

/// The acts placed in the predicted final block's slots.
async fn timeplan_acts(app: &TestApp) -> Vec<Value> {
    finalists(&predicted_items(app).await[0])
        .iter()
        .map(|finalist| finalist["act"]["id"].clone())
        .collect()
}

#[tokio::test]
async fn placed_acts_show_once_the_placement_is_published() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    // Both start in EWU13, a category of the final.
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let berta = app.add_single_act(&owner, club_id, "Berta").await;
    replace_timeplan(&app, &admin, &[final_entry(FINAL, Some(START))]).await;
    let slots = slot_ids(&app).await;

    let response = place(
        &app,
        &admin,
        json!([
            { "slot_id": slots[0], "act_id": anna },
            { "slot_id": slots[1], "act_id": berta },
        ]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        placed(&app, Some(&admin)).await,
        [json!(anna), json!(berta), Value::Null]
    );

    // Not published yet: only admins see the placement.
    assert_eq!(placed(&app, None).await, vec![Value::Null; 3]);
    assert_eq!(placed(&app, Some(&owner)).await, vec![Value::Null; 3]);
    assert_eq!(timeplan_acts(&app).await, vec![Value::Null; 3]);
    assert!(
        !list_finals(&app).await[0]["placement_published"]
            .as_bool()
            .unwrap()
    );

    let response = publish(&app, &admin, true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        placed(&app, None).await,
        [json!(anna), json!(berta), Value::Null]
    );
    assert_eq!(
        timeplan_acts(&app).await,
        [json!(anna), json!(berta), Value::Null]
    );
    let finalist = finalists(&predicted_items(&app).await[0]).remove(0);
    assert_eq!(finalist["label"], "1. Finalist (JE EW)");
    assert_eq!(finalist["act"]["category"], "EWU13");
    assert_eq!(
        finalist["act"]["category_description"],
        "Einzel Weiblich U13"
    );

    // Acts swap slots in one go; the other slots stay.
    let response = place(
        &app,
        &admin,
        json!([
            { "slot_id": slots[0], "act_id": berta },
            { "slot_id": slots[1], "act_id": anna },
        ]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        timeplan_acts(&app).await,
        [json!(berta), json!(anna), Value::Null]
    );

    // An act takes one slot of a final.
    rejected(
        &place(
            &app,
            &admin,
            json!([{ "slot_id": slots[0], "act_id": anna }]),
        )
        .await,
        "nur einen Finalplatz",
    );
    // A slot takes only acts of its heat: Carla starts in EWU13, a
    // category of "JE EW", not of "E EW".
    let carla = app.add_single_act(&owner, club_id, "Carla").await;
    rejected(
        &place(
            &app,
            &admin,
            json!([{ "slot_id": slots[2], "act_id": carla }]),
        )
        .await,
        "nicht zur Vorlaufklasse „E EW“",
    );
    // Acts placed before stay when their category leaves the heat.
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "name": "JE EW", "new_name": "JE EW", "description": null, "categories": ["EWU11"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        timeplan_acts(&app).await,
        [json!(berta), json!(anna), Value::Null]
    );
    // Slots of other finals are refused.
    rejected(
        &place(&app, &admin, json!([{ "slot_id": anna, "act_id": null }])).await,
        "gibt es in diesem Finale nicht",
    );

    // Emptying a slot and withdrawing the publication.
    let response = place(
        &app,
        &admin,
        json!([{ "slot_id": slots[1], "act_id": null }]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        timeplan_acts(&app).await,
        [json!(berta), Value::Null, Value::Null]
    );
    let response = publish(&app, &admin, false).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(timeplan_acts(&app).await, vec![Value::Null; 3]);

    // Only admins place and publish.
    assert_eq!(
        place(
            &app,
            &owner,
            json!([{ "slot_id": slots[1], "act_id": anna }])
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        publish(&app, &owner, true).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn placements_survive_new_slot_groups() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let berta = app.add_single_act(&owner, club_id, "Berta").await;
    let slots = slot_ids(&app).await;
    let response = place(
        &app,
        &admin,
        json!([
            { "slot_id": slots[0], "act_id": berta },
            { "slot_id": slots[1], "act_id": anna },
        ]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // "1. Finalist (JE EW)" is still there, now second; "2. Finalist (JE EW)"
    // isn't.
    let mut body = final_body(
        FINAL,
        json!([{ "heat": "E EW", "count": 1 }, { "heat": "JE EW", "count": 1 }]),
    );
    body["new_name"] = json!(FINAL);
    body["is_single_male"] = json!(false);
    let response = app
        .post("/api/command/edit_final", body, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        placed(&app, Some(&admin)).await,
        [Value::Null, json!(berta)]
    );
    assert_eq!(
        list_finals(&app).await[0]["categories"],
        json!(["EWU11", "EWU13", "EWU15", "EWU17"])
    );
}

#[tokio::test]
async fn a_placed_act_plays_in_its_final_slot() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    replace_timeplan(&app, &admin, &[final_entry(FINAL, Some(START))]).await;
    let slots = slot_ids(&app).await;
    let response = place(
        &app,
        &admin,
        json!([{ "slot_id": slots[0], "act_id": anna }]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(publish(&app, &admin, true).await.status, StatusCode::OK);

    let music_started =
        async || finalists(&predicted_items(&app).await[0])[0]["music_started_at"].clone();
    let report = async || {
        let response = app
            .post(
                "/api/command/set_act_music_started",
                json!({ "act_id": anna }),
                Some(&admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    };

    // The warm-up: the slot doesn't run yet.
    step(&app, &admin, "timeplan_forward").await;
    report().await;
    assert!(music_started().await.is_null());

    // The slot runs: its timer counts from the music's start. The act's own
    // heat stays untouched.
    step(&app, &admin, "timeplan_forward").await;
    report().await;
    assert!(!music_started().await.is_null());
    let heat: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT started_at, music_started_at FROM acts WHERE id = ?")
            .bind(anna)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(heat, (None, None));

    // A started slot keeps its act, and the placement stays published.
    rejected(
        &place(
            &app,
            &admin,
            json!([{ "slot_id": slots[0], "act_id": null }]),
        )
        .await,
        "schon begonnen",
    );
    rejected(&publish(&app, &admin, false).await, "schon begonnen");

    // Taking the start back forgets the music start.
    step(&app, &admin, "timeplan_backward").await;
    assert!(music_started().await.is_null());
}
