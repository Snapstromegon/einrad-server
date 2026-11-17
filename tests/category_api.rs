//! The API addresses categories by `category_id` (#356, ADR 0018); a rename
//! is a plain update that keeps everything attached.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::ids::CategoryId;
use serde_json::{Value, json};
use uuid::Uuid;

fn category_body(name: &str) -> Value {
    json!({
        "name": name,
        "description": "Testkategorie",
        "min_age": 17,
        "under_age": 27.5,
        "act_type": "pair",
        "is_single_male": false,
        "einfahrzeit_seconds": 300,
        "act_duration_seconds": 120,
        "judge_duration_seconds": 60,
    })
}

/// A body for `edit_category` that sends the ID and keeps the category's
/// ages and discipline (EWU13 is in a heat), as the frontend does.
async fn edit_body(app: &TestApp, category_id: CategoryId, new_name: &str) -> Value {
    let list = app.get("/api/query/list_categories", None).await;
    let current = list
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == category_id.to_string())
        .cloned()
        .unwrap_or_else(|| category_body("Egal"));
    json!({
        "category_id": category_id,
        "new_name": new_name,
        "description": current["description"],
        "min_age": current["min_age"],
        "under_age": current["under_age"],
        "act_type": current["act_type"],
        "is_single_male": current["is_single_male"],
        "einfahrzeit_seconds": current["einfahrzeit_seconds"],
        "act_duration_seconds": current["act_duration_seconds"],
        "judge_duration_seconds": current["judge_duration_seconds"],
    })
}

#[tokio::test]
async fn a_new_category_gets_a_v7_id_and_the_list_shows_it() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let added = app
        .post(
            "/api/command/add_category",
            category_body("TEST"),
            Some(&admin),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let id: CategoryId = added.body["category_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(id.0.get_version_num(), 7);

    let list = app.get("/api/query/list_categories", None).await;
    let found = list
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "TEST")
        .unwrap();
    assert_eq!(found["id"], id.to_string());
}

#[tokio::test]
async fn renaming_a_category_keeps_its_timeplan_startlist_and_riders() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let id = app.category_id("EWU13").await;

    let entry = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": "2026-04-19T09:00:00Z",
                "category_id": id,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(entry.status, StatusCode::OK, "{:?}", entry.body);

    let renamed = app
        .post(
            "/api/command/edit_category",
            edit_body(&app, id, "EWU13 neu").await,
            Some(&admin),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);

    // Same ID, new name; no category of the old name is left.
    assert_eq!(app.category_id("EWU13 neu").await, id);
    let old: Option<CategoryId> =
        sqlx::query_scalar("SELECT id FROM categories WHERE name = 'EWU13'")
            .fetch_optional(&app.db)
            .await
            .unwrap();
    assert_eq!(old, None);

    // The timeplan entry still points at it.
    let timeplan = app.get("/api/query/list_timeplan", Some(&admin)).await;
    let block = timeplan
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == entry.body["id"])
        .unwrap();
    assert_eq!(block["category_id"], id.to_string());
    assert_eq!(block["category"], "EWU13 neu");

    // The act, with its rider, is still in the category; so is the startlist.
    let act = app
        .get(&format!("/api/query/get_act?act_id={anna}"), Some(&admin))
        .await;
    assert_eq!(act.body["category_id"], id.to_string(), "{:?}", act.body);
    assert_eq!(act.body["category"], "EWU13 neu");
    assert_eq!(act.body["participants"].as_array().unwrap().len(), 1);
    let startlist = app.get("/api/query/startlist", Some(&admin)).await;
    let row = startlist
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == anna.to_string())
        .unwrap();
    assert_eq!(row["category_id"], id.to_string());
    assert_eq!(row["category"], "EWU13 neu");

    // The predicted timeplan runs the act in the renamed category's block.
    let predicted = app.get("/api/query/predict_timeplan", None).await;
    let text = predicted.body.to_string();
    assert!(text.contains(&anna.to_string()), "{text}");
    assert!(text.contains(&id.to_string()), "{text}");
}

#[tokio::test]
async fn a_rename_to_a_taken_name_is_refused_and_an_unknown_id_is_a_404() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let id = app.category_id("EWU13").await;

    let taken = app
        .post(
            "/api/command/edit_category",
            edit_body(&app, id, "EMU13").await,
            Some(&admin),
        )
        .await;
    assert_ne!(taken.status, StatusCode::OK, "{:?}", taken.body);
    assert_eq!(app.category_id("EWU13").await, id);

    let unknown = app
        .post(
            "/api/command/edit_category",
            edit_body(&app, CategoryId::new(), "Egal").await,
            Some(&admin),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{:?}", unknown.body);
}

#[tokio::test]
async fn categories_are_deleted_and_moved_by_id() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let ewu13 = app.category_id("EWU13").await;
    let order_of = |name: &'static str| {
        let app = &app;
        async move {
            sqlx::query_scalar::<_, i64>("SELECT \"order\" FROM categories WHERE name = $1")
                .bind(name)
                .fetch_one(&app.db)
                .await
                .unwrap()
        }
    };

    let before = order_of("EWU13").await;
    let up = app
        .post(
            "/api/command/move_category_up",
            json!({ "category_id": ewu13 }),
            Some(&admin),
        )
        .await;
    assert_eq!(up.status, StatusCode::OK, "{:?}", up.body);
    assert_eq!(order_of("EWU13").await, before - 1);
    let down = app
        .post(
            "/api/command/move_category_down",
            json!({ "category_id": ewu13 }),
            Some(&admin),
        )
        .await;
    assert_eq!(down.status, StatusCode::OK, "{:?}", down.body);
    assert_eq!(order_of("EWU13").await, before);

    let deleted = app
        .post(
            "/api/command/delete_category",
            json!({ "category_id": ewu13 }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    let again = app
        .post(
            "/api/command/delete_category",
            json!({ "category_id": ewu13 }),
            Some(&admin),
        )
        .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND, "{:?}", again.body);
}

#[tokio::test]
async fn heats_take_category_ids_and_list_them_next_to_the_names() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let emu19 = app.category_id("EMU19").await;
    let emu21 = app.category_id("EMU21").await;

    let added = app
        .post(
            "/api/command/add_heat",
            json!({
                "name": "Neue Klasse",
                "description": null,
                "category_ids": [emu19, emu21, emu19],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let heats = app.get("/api/query/list_heats", None).await;
    let heat = heats
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "Neue Klasse")
        .unwrap();
    assert_eq!(heat["categories"], json!(["EMU19", "EMU21"]));
    assert_eq!(heat["category_ids"], json!([emu19, emu21]));

    let unknown = app
        .post(
            "/api/command/add_heat",
            json!({
                "name": "Geisterklasse",
                "description": null,
                "category_ids": [Uuid::now_v7()],
            }),
            Some(&admin),
        )
        .await;
    assert_ne!(unknown.status, StatusCode::OK, "{:?}", unknown.body);
}

#[tokio::test]
async fn edit_heat_and_edit_timeplan_entry_take_category_ids() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let emu19 = app.category_id("EMU19").await;
    let emu21 = app.category_id("EMU21").await;

    let added = app
        .post(
            "/api/command/add_heat",
            json!({"name": "Klasse", "description": null, "category_ids": [emu19]}),
            Some(&admin),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let heats = app.get("/api/query/list_heats", None).await;
    let heat_id = heats
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "Klasse")
        .unwrap()["id"]
        .clone();

    let edited = app
        .post(
            "/api/command/edit_heat",
            json!({
                "id": heat_id,
                "new_name": "Klasse",
                "description": null,
                "category_ids": [emu21],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let heats = app.get("/api/query/list_heats", None).await;
    let heat = heats
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == heat_id)
        .unwrap();
    assert_eq!(heat["category_ids"], json!([emu21]));

    let entry = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": "2026-04-19T09:00:00Z",
                "category_id": emu19,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(entry.status, StatusCode::OK, "{:?}", entry.body);
    let edited = app
        .post(
            "/api/command/edit_timeplan_entry",
            json!({
                "id": entry.body["id"],
                "earliest_start_time": "2026-04-19T09:00:00Z",
                "category_id": emu21,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let timeplan = app.get("/api/query/list_timeplan", Some(&admin)).await;
    let block = timeplan
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == entry.body["id"])
        .unwrap();
    assert_eq!(block["category_id"], json!(emu21));
}
