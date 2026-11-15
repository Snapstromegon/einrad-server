//! Integration tests for the structure tree (#346): discipline, heat,
//! category, act, in one consistent read.

mod common;

use axum::http::StatusCode;
use common::{TestApp, rider_body};
use einrad_server::ids::ActId;
use serde_json::{Value, json};

async fn structure(app: &TestApp, cookie: &str) -> Value {
    let response = app.get("/api/query/get_structure", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

/// The discipline node, e.g. `("single", false)` for Einzel weiblich.
fn discipline<'a>(tree: &'a Value, act_type: &str, single_male: bool) -> &'a Value {
    tree["disciplines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["act_type"] == act_type && d["is_single_male"] == single_male)
        .unwrap_or_else(|| panic!("no discipline {act_type} {single_male}"))
}

fn named<'a>(nodes: &'a Value, name: &str) -> &'a Value {
    nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == name)
        .unwrap_or_else(|| panic!("no node {name} in {nodes}"))
}

fn names(nodes: &Value) -> Vec<&str> {
    nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect()
}

/// The heat `name` inside the final `final_name` of a discipline.
fn heat_in<'a>(discipline: &'a Value, final_name: &str, name: &str) -> &'a Value {
    named(&named(&discipline["finals"], final_name)["heats"], name)
}

async fn post_ok(app: &TestApp, admin: &str, command: &str, body: Value) {
    let response = app
        .post(&format!("/api/command/{command}"), body, Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn set_act_order(app: &TestApp, admin: &str, act_id: ActId, order: i64) {
    let response = app
        .post(
            "/api/command/set_act_order",
            json!({ "act_id": act_id, "order": order }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn the_tree_has_the_five_disciplines_in_act_type_order() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let tree = structure(&app, &admin).await;
    let order: Vec<(&str, bool)> = tree["disciplines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["act_type"].as_str().unwrap(),
                d["is_single_male"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("single", false),
            ("single", true),
            ("pair", false),
            ("group_small", false),
            ("group_large", false),
        ]
    );
}

#[tokio::test]
async fn only_admins_planning_the_competition_read_the_tree() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    assert_ne!(
        app.get("/api/query/get_structure", None).await.status,
        StatusCode::OK
    );
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    assert_ne!(
        app.get("/api/query/get_structure", Some(&owner))
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_final_shows_its_slots_and_its_heats_in_slot_group_order() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let tree = structure(&app, &admin).await;
    let women = discipline(&tree, "single", false);

    // Fixture: the final takes 2 slots of JE EW, then 1 of E EW.
    assert_eq!(names(&women["finals"]), ["Finale EW"]);
    let final_node = named(&women["finals"], "Finale EW");
    assert_eq!(final_node["slots"], 3);
    assert_eq!(names(&final_node["heats"]), ["JE EW", "E EW"]);
    let je = heat_in(women, "Finale EW", "JE EW");
    assert_eq!(je["slots"], 2);
    assert_eq!(je["skip"], false);
    assert_eq!(names(&je["categories"]), ["EWU11", "EWU13", "EWU15"]);
    assert_eq!(heat_in(women, "Finale EW", "E EW")["slots"], 1);
    assert_eq!(women["heats_without_final"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_final_fed_by_two_heats_appears_once_with_all_its_acts() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let mut body = rider_body(club_id, "Max", true, false);
    body["single_male"] = json!(true);
    body["single_female"] = json!(false);
    body["gender"] = json!("male");
    let rider = app.add_rider(&owner, body).await;
    let act = app.act_of(rider).await;
    for (name, category) in [("E EM 1", "EMU13"), ("E EM 2", "EMU19")] {
        post_ok(
            &app,
            &admin,
            "add_heat",
            json!({ "name": name, "description": null, "categories": [category] }),
        )
        .await;
    }
    post_ok(
        &app,
        &admin,
        "add_final",
        json!({
            "name": "Finale E EM",
            "description": null,
            "act_type": "single",
            "is_single_male": true,
            "einfahrzeit_seconds": 600,
            "act_duration_seconds": 300,
            "judge_duration_seconds": 180,
            "slot_groups": [
                { "heat_id": app.heat_id("E EM 2").await, "count": 1 },
                { "heat_id": app.heat_id("E EM 1").await, "count": 3 },
            ],
        }),
    )
    .await;

    let tree = structure(&app, &admin).await;
    let men = discipline(&tree, "single", true);
    assert_eq!(names(&men["finals"]), ["Finale E EM"]);
    let final_node = named(&men["finals"], "Finale E EM");
    assert_eq!(final_node["slots"], 4);
    // In slot group order, not by name.
    assert_eq!(names(&final_node["heats"]), ["E EM 2", "E EM 1"]);
    assert_eq!(heat_in(men, "Finale E EM", "E EM 1")["slots"], 3);
    assert_eq!(heat_in(men, "Finale E EM", "E EM 2")["slots"], 1);
    // All acts of its heats: Max is in EMU13 of "E EM 1".
    assert_eq!(final_node["act_count"], 1);
    assert_eq!(heat_in(men, "Finale E EM", "E EM 1")["act_count"], 1);
    assert_eq!(heat_in(men, "Finale E EM", "E EM 2")["act_count"], 0);
    let category = named(
        &heat_in(men, "Finale E EM", "E EM 1")["categories"],
        "EMU13",
    );
    assert_eq!(category["acts"][0]["id"], act.to_string());
    assert_eq!(men["heats_without_final"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn final_and_heat_nodes_carry_their_readable_name_and_code() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    post_ok(
        &app,
        &admin,
        "add_heat",
        json!({
            "name": "Einzel männlich – Expert 1", "code": "E EM 1",
            "description": null, "categories": ["EMU13"],
        }),
    )
    .await;
    post_ok(
        &app,
        &admin,
        "add_final",
        json!({
            "name": "Finale Einzel männlich Expert", "code": "E EM",
            "description": null, "act_type": "single", "is_single_male": true,
            "einfahrzeit_seconds": 600, "act_duration_seconds": 300,
            "judge_duration_seconds": 180,
            "slot_groups": [{ "heat_id": app.heat_id("E EM 1").await, "count": 2 }],
        }),
    )
    .await;

    let tree = structure(&app, &admin).await;
    let final_node = named(
        &discipline(&tree, "single", true)["finals"],
        "Finale Einzel männlich Expert",
    );
    assert_eq!(final_node["code"], "E EM");
    let heat = named(&final_node["heats"], "Einzel männlich – Expert 1");
    assert_eq!(heat["code"], "E EM 1");
}

#[tokio::test]
async fn a_heat_no_final_takes_is_listed_apart() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    post_ok(
        &app,
        &admin,
        "add_heat",
        json!({ "name": "Nur Männer", "description": null, "categories": ["EMU11"] }),
    )
    .await;
    let tree = structure(&app, &admin).await;
    let men = discipline(&tree, "single", true);
    assert_eq!(men["finals"].as_array().unwrap().len(), 0);
    let heat = named(&men["heats_without_final"], "Nur Männer");
    assert_eq!(heat["slots"], Value::Null);
    assert_eq!(names(&heat["categories"]), ["EMU11"]);
}

#[tokio::test]
async fn categories_without_a_heat_are_listed_apart() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let tree = structure(&app, &admin).await;
    let women = discipline(&tree, "single", false);
    let unassigned = names(&women["without_heat"]);
    assert!(unassigned.contains(&"EWU19"), "{unassigned:?}");
    assert!(!unassigned.contains(&"EWU13"), "{unassigned:?}");
    // No heat of men exists in the fixture: all their categories are there.
    let men = discipline(&tree, "single", true);
    assert_eq!(men["finals"].as_array().unwrap().len(), 0);
    assert_eq!(men["heats_without_final"].as_array().unwrap().len(), 0);
    assert!(names(&men["without_heat"]).contains(&"EMU11"));
}

#[tokio::test]
async fn acts_come_in_timeplan_order_with_unplaced_ones_after_by_name() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let mut ids = Vec::new();
    for name in ["Berta", "Anna", "Carla", "Dora"] {
        ids.push((name, app.add_single_act(&owner, club_id, name).await));
    }
    // Dora starts first, Carla second; Anna and Berta are not placed.
    set_act_order(&app, &admin, ids[3].1, 1).await;
    set_act_order(&app, &admin, ids[2].1, 2).await;

    let tree = structure(&app, &admin).await;
    let women = discipline(&tree, "single", false);
    let category = named(&heat_in(women, "Finale EW", "JE EW")["categories"], "EWU13");
    let riders: Vec<&str> = category["acts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["participants"][0]["firstname"].as_str().unwrap())
        .collect();
    assert_eq!(riders, ["Dora", "Carla", "Anna", "Berta"]);
    let first = &category["acts"][0];
    assert_eq!(first["id"], ids[3].1.to_string());
    assert_eq!(first["participants"][0]["club_name"], "RSV");
    assert_eq!(first["withdrawn"], false);
    assert_eq!(category["act_count"], 4);
    assert_eq!(heat_in(women, "Finale EW", "JE EW")["act_count"], 4);
    assert_eq!(women["act_count"], 4);
}

#[tokio::test]
async fn an_incomplete_group_has_no_category() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let group = app.create_group(&owner, club_id, "Die Rollenden").await;

    let tree = structure(&app, &admin).await;
    let groups = discipline(&tree, "group_small", false);
    let without = &groups["without_category"];
    assert_eq!(without["act_count"], 1);
    assert_eq!(without["acts"][0]["id"], group.to_string());
    assert_eq!(without["acts"][0]["group_name"], "Die Rollenden");
    // Nothing of it in the other disciplines.
    assert_eq!(
        discipline(&tree, "group_large", false)["without_category"]["act_count"],
        0
    );
}

#[tokio::test]
async fn withdrawn_acts_are_marked_and_not_counted() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    app.add_single_act(&owner, club_id, "Berta").await;
    let response = app
        .post(
            "/api/command/set_act_withdrawn",
            json!({ "act_id": anna, "withdrawn": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let tree = structure(&app, &admin).await;
    let women = discipline(&tree, "single", false);
    // A withdrawn act keeps its category, marked, and doesn't count.
    assert_eq!(
        women["without_category"]["acts"].as_array().unwrap().len(),
        0
    );
    let category = named(&heat_in(women, "Finale EW", "JE EW")["categories"], "EWU13");
    let acts = category["acts"].as_array().unwrap();
    assert_eq!(acts.len(), 2);
    let withdrawn = acts.iter().find(|a| a["id"] == anna.to_string()).unwrap();
    assert_eq!(withdrawn["withdrawn"], true);
    assert_eq!(women["act_count"], 1);
    assert_eq!(category["act_count"], 1);
}

#[tokio::test]
async fn a_pair_without_a_heat_sits_in_its_category_without_heat() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let carla = app
        .add_rider(&owner, rider_body(club_id, "Carla", false, true))
        .await;
    let mut dora = rider_body(club_id, "Dora", false, true);
    dora["partner_id"] = json!(carla);
    app.add_rider(&owner, dora).await;

    let tree = structure(&app, &admin).await;
    let pairs = discipline(&tree, "pair", false);
    let category = named(&pairs["without_heat"], "PU13");
    assert_eq!(category["act_count"], 1);
    assert_eq!(
        category["acts"][0]["participants"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(pairs["act_count"], 1);
}

#[tokio::test]
async fn a_final_of_skipped_heats_has_no_slot_count() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    post_ok(
        &app,
        &admin,
        "add_heat",
        json!({ "name": "Kleingruppe Expert", "description": null, "categories": ["KG15+"] }),
    )
    .await;
    post_ok(
        &app,
        &admin,
        "add_final",
        json!({
            "name": "Finale KG Expert",
            "description": null,
            "act_type": "group_small",
            "is_single_male": false,
            "einfahrzeit_seconds": 600,
            "act_duration_seconds": 300,
            "judge_duration_seconds": 180,
            "slot_groups": [{ "heat_id": app.heat_id("Kleingruppe Expert").await, "count": 1 }],
        }),
    )
    .await;

    let tree = structure(&app, &admin).await;
    let groups = discipline(&tree, "group_small", false);
    // All its heats are skipped: the final's slot count means nothing.
    let final_node = named(&groups["finals"], "Finale KG Expert");
    assert_eq!(final_node["slots"], Value::Null);
    assert_eq!(final_node["act_count"], 0);
    let heat = heat_in(groups, "Finale KG Expert", "Kleingruppe Expert");
    assert_eq!(heat["skip"], true);
    assert_eq!(names(&heat["categories"]), ["KG15+"]);
    assert!(names(&groups["without_heat"]).contains(&"KGU15"));
}

#[tokio::test]
async fn a_final_counts_only_the_slots_of_heats_that_run_their_vorlauf() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    for (name, category) in [("E EM 1", "EMU13"), ("E EM 2", "EMU19")] {
        post_ok(
            &app,
            &admin,
            "add_heat",
            json!({ "name": name, "description": null, "categories": [category] }),
        )
        .await;
    }
    post_ok(
        &app,
        &admin,
        "add_final",
        json!({
            "name": "Finale E EM",
            "description": null,
            "act_type": "single",
            "is_single_male": true,
            "einfahrzeit_seconds": 600,
            "act_duration_seconds": 300,
            "judge_duration_seconds": 180,
            "slot_groups": [
                { "heat_id": app.heat_id("E EM 1").await, "count": 4 },
                { "heat_id": app.heat_id("E EM 2").await, "count": 4 },
            ],
        }),
    )
    .await;
    post_ok(
        &app,
        &admin,
        "set_heat_skip",
        json!({ "id": app.heat_id("E EM 2").await, "skip": true }),
    )
    .await;

    let tree = structure(&app, &admin).await;
    let men = discipline(&tree, "single", true);
    // The skipped heat's 4 slots are unused while it is skipped.
    assert_eq!(named(&men["finals"], "Finale E EM")["slots"], 4);
}
