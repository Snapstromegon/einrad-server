//! Integration tests for the structure tree (#346): discipline, heat,
//! category, act, in one consistent read.

mod common;

use axum::http::StatusCode;
use common::{TestApp, rider_body};
use serde_json::{Value, json};
use uuid::Uuid;

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

async fn set_act_order(app: &TestApp, admin: &str, act_id: Uuid, order: i64) {
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
async fn a_heat_shows_its_final_slots_skip_and_categories_in_order() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let tree = structure(&app, &admin).await;
    let women = discipline(&tree, "single", false);

    let heats = &women["heats"];
    assert_eq!(names(heats), ["JE EW", "E EW"]);
    let je = named(heats, "JE EW");
    // Fixture: the final takes 2 slots of JE EW and 1 of E EW.
    assert_eq!(je["final_name"], "Finale EW");
    assert_eq!(je["slots"], 2);
    assert_eq!(je["skip"], false);
    assert_eq!(names(&je["categories"]), ["EWU11", "EWU13", "EWU15"]);
    assert_eq!(named(heats, "E EW")["slots"], 1);
}

#[tokio::test]
async fn a_heat_without_a_final_has_none() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/add_heat",
            json!({ "name": "Nur Männer", "description": null, "categories": ["EMU11"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let tree = structure(&app, &admin).await;
    let heat = named(&discipline(&tree, "single", true)["heats"], "Nur Männer");
    assert_eq!(heat["final_name"], Value::Null);
    assert_eq!(heat["slots"], Value::Null);
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
    assert_eq!(men["heats"].as_array().unwrap().len(), 0);
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
    let category = named(&named(&women["heats"], "JE EW")["categories"], "EWU13");
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
    assert_eq!(named(&women["heats"], "JE EW")["act_count"], 4);
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
    // A withdrawn act has no category: it sits in "Ohne Kategorie" of its
    // discipline, marked, and doesn't count.
    let without = &women["without_category"];
    assert_eq!(without["act_count"], 0);
    assert_eq!(without["acts"].as_array().unwrap().len(), 1);
    assert_eq!(without["acts"][0]["id"], anna.to_string());
    assert_eq!(without["acts"][0]["withdrawn"], true);
    assert_eq!(women["act_count"], 1);
    let category = named(&named(&women["heats"], "JE EW")["categories"], "EWU13");
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
async fn a_group_discipline_shows_its_skipped_heat_and_final() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    for (command, body) in [
        (
            "add_heat",
            json!({ "name": "Kleingruppe Expert", "description": null, "categories": ["KG15+"] }),
        ),
        (
            "add_final",
            json!({
                "name": "Finale KG Expert",
                "description": null,
                "act_type": "group_small",
                "is_single_male": false,
                "einfahrzeit_seconds": 600,
                "act_duration_seconds": 300,
                "judge_duration_seconds": 180,
                "slot_groups": [{ "heat": "Kleingruppe Expert", "count": 1 }],
            }),
        ),
    ] {
        let response = app
            .post(&format!("/api/command/{command}"), body, Some(&admin))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    let tree = structure(&app, &admin).await;
    let groups = discipline(&tree, "group_small", false);
    let heat = named(&groups["heats"], "Kleingruppe Expert");
    assert_eq!(heat["skip"], true);
    assert_eq!(heat["final_name"], "Finale KG Expert");
    assert_eq!(names(&heat["categories"]), ["KG15+"]);
    assert!(names(&groups["without_heat"]).contains(&"KGU15"));
}
