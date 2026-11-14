//! Integration tests for heats (#338): named groups of categories of one
//! discipline whose best acts fill a final's finalist slots.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

/// The last migration before heats.
const BEFORE_HEATS: i64 = 20261111120000;

async fn migrated_before_heats() -> SqlitePool {
    let db = common::in_memory().await;
    sqlx::migrate!("./migrations")
        .run_to(BEFORE_HEATS, &db)
        .await
        .unwrap();
    db
}

/// Inserts a category with its age class: "U17" is under 17, "70+" at
/// least 70.
async fn insert_category(
    db: &SqlitePool,
    name: &str,
    act_type: &str,
    single_male: bool,
    min_age: Option<f64>,
    under_age: Option<f64>,
) {
    sqlx::query(
        "INSERT INTO categories (name, act_type, is_single_male, min_age, under_age) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind(act_type)
    .bind(single_male)
    .bind(min_age)
    .bind(under_age)
    .execute(db)
    .await
    .unwrap();
}

/// Inserts a final as before heats: slot groups by label, its categories
/// by hand.
async fn insert_old_final(
    db: &SqlitePool,
    name: &str,
    order: i64,
    act_type: &str,
    single_male: bool,
    groups: &[(&str, i64)],
    categories: &[&str],
) {
    sqlx::query(
        "INSERT INTO finals (name, act_type, is_single_male, \"order\") VALUES (?, ?, ?, ?)",
    )
    .bind(name)
    .bind(act_type)
    .bind(single_male)
    .bind(order)
    .execute(db)
    .await
    .unwrap();
    let mut position = 0;
    for (label, count) in groups {
        for number in 1..=*count {
            position += 1;
            sqlx::query(
                "INSERT INTO final_slots (id, final_name, position, group_label, number) \
                 VALUES (randomblob(16), ?, ?, ?, ?)",
            )
            .bind(name)
            .bind(position)
            .bind(label)
            .bind(number)
            .execute(db)
            .await
            .unwrap();
        }
    }
    for category in categories {
        sqlx::query("INSERT INTO final_categories (final_name, category) VALUES (?, ?)")
            .bind(name)
            .bind(category)
            .execute(db)
            .await
            .unwrap();
    }
}

/// The singles male categories of the LM: U11 … U70 and 70+.
async fn insert_lm_categories(db: &SqlitePool) {
    let mut min = None;
    for under in [
        11.0, 13.0, 15.0, 17.0, 19.0, 21.0, 23.0, 25.0, 30.0, 50.0, 70.0,
    ] {
        insert_category(db, &format!("EMU{under}"), "single", true, min, Some(under)).await;
        min = Some(under);
    }
    insert_category(db, "EM70+", "single", true, Some(70.0), None).await;
}

/// The heats with their categories (by name), in name order.
async fn heats(db: &SqlitePool) -> Vec<(String, Option<String>, Vec<String>)> {
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT heats.name, heats.description, \
         (SELECT group_concat(name, ',') FROM (SELECT name FROM categories \
            WHERE heat_id = heats.id ORDER BY name)) \
         FROM heats ORDER BY heats.name",
    )
    .fetch_all(db)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(name, description, categories)| {
            let categories = categories
                .map(|c| c.split(',').map(String::from).collect())
                .unwrap_or_default();
            (name, description, categories)
        })
        .collect()
}

/// The heat of each slot of a final, in start order.
async fn slot_heats(db: &SqlitePool, final_name: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT heats.name FROM final_slots \
         JOIN finals ON finals.id = final_slots.final_id \
         JOIN heats ON heats.id = final_slots.heat_id \
         WHERE finals.name = ? ORDER BY position",
    )
    .bind(final_name)
    .fetch_all(db)
    .await
    .unwrap()
}

fn heat(name: &str, categories: &[&str]) -> (String, Option<String>, Vec<String>) {
    (
        name.to_string(),
        None,
        categories.iter().map(|c| c.to_string()).collect(),
    )
}

#[tokio::test]
async fn the_migration_builds_heats_from_the_slot_labels() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_old_final(
        &db,
        "JE EM",
        1,
        "single",
        true,
        &[("U11–U15", 5)],
        &["EMU11", "EMU13", "EMU15"],
    )
    .await;
    insert_old_final(
        &db,
        "E EM",
        2,
        "single",
        true,
        &[("U17–U21", 2), ("U23+", 2)],
        &[
            "EM70+", "EMU17", "EMU19", "EMU21", "EMU23", "EMU25", "EMU30", "EMU50", "EMU70",
        ],
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    assert_eq!(
        heats(&db).await,
        [
            heat("E EM 1", &["EMU17", "EMU19", "EMU21"]),
            heat(
                "E EM 2",
                &["EM70+", "EMU23", "EMU25", "EMU30", "EMU50", "EMU70"]
            ),
            heat("JE EM", &["EMU11", "EMU13", "EMU15"]),
        ]
    );
    assert_eq!(slot_heats(&db, "JE EM").await, ["JE EM"; 5]);
    assert_eq!(
        slot_heats(&db, "E EM").await,
        ["E EM 1", "E EM 1", "E EM 2", "E EM 2"]
    );
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(!tables.contains(&"final_categories".to_string()));
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('final_slots')")
            .fetch_all(&db)
            .await
            .unwrap();
    assert!(!columns.contains(&"group_label".to_string()), "{columns:?}");
}

#[tokio::test]
async fn a_single_slot_group_takes_all_categories_of_its_final() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_old_final(
        &db,
        "Finale",
        1,
        "single",
        true,
        &[("Die Besten", 3)],
        &["EMU11", "EMU13"],
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    assert_eq!(heats(&db).await, [heat("Finale", &["EMU11", "EMU13"])]);
}

#[tokio::test]
async fn a_label_matching_nothing_gives_an_empty_heat() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_old_final(
        &db,
        "E EM",
        1,
        "single",
        true,
        &[("U17-U21", 2), ("U23 und älter", 2)],
        &["EMU17", "EMU19", "EMU21", "EMU23", "EMU25"],
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    // A hyphen works like the en dash; "und älter" isn't understood.
    assert_eq!(
        heats(&db).await,
        [
            heat("E EM 1", &["EMU17", "EMU19", "EMU21"]),
            heat("E EM 2", &[]),
        ]
    );
}

#[tokio::test]
async fn on_an_overlap_the_first_match_wins() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_old_final(&db, "B", 2, "single", true, &[("U11", 1)], &["EMU11"]).await;
    insert_old_final(
        &db,
        "A",
        1,
        "single",
        true,
        &[("U11–U13", 1), ("U11", 1)],
        &["EMU11", "EMU13"],
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    assert_eq!(
        heats(&db).await,
        [
            heat("A 1", &["EMU11", "EMU13"]),
            heat("A 2", &[]),
            heat("B", &[]),
        ]
    );
}

#[tokio::test]
async fn only_categories_of_the_finals_discipline_join_its_heats() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_category(&db, "EWU11", "single", false, None, Some(11.0)).await;
    insert_category(&db, "PU11", "pair", false, None, Some(11.0)).await;
    insert_old_final(
        &db,
        "JE EM",
        1,
        "single",
        true,
        &[("U11", 1)],
        &["EMU11", "EWU11", "PU11"],
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    assert_eq!(heats(&db).await, [heat("JE EM", &["EMU11"])]);
}

#[tokio::test]
async fn reverting_the_migration_writes_labels_and_categories_back() {
    let db = migrated_before_heats().await;
    insert_lm_categories(&db).await;
    insert_old_final(
        &db,
        "E EM",
        1,
        "single",
        true,
        &[("U17–U21", 1), ("U23+", 1)],
        &["EMU17", "EMU23"],
    )
    .await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run(&db).await.unwrap();

    migrator.undo(&db, BEFORE_HEATS).await.unwrap();

    let labels: Vec<String> = sqlx::query_scalar(
        "SELECT group_label FROM final_slots WHERE final_name = 'E EM' ORDER BY position",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(labels, ["E EM 1", "E EM 2"]);
    let categories: Vec<String> = sqlx::query_scalar(
        "SELECT category FROM final_categories WHERE final_name = 'E EM' ORDER BY category",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(categories, ["EMU17", "EMU23"]);
}

// The admin API. The fixture has the heats "JE EW" (EWU11, EWU13, EWU15) and
// "E EW" (EWU17), both in the final "Finale EW".

async fn list_heats(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_heats", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

fn heat_body(name: &str, categories: &[&str]) -> Value {
    json!({ "name": name, "description": null, "categories": categories })
}

fn edit_body(id: Uuid, new_name: &str, categories: &[&str]) -> Value {
    json!({
        "id": id,
        "new_name": new_name,
        "description": "Expert 1 Einzel Männlich",
        "categories": categories,
    })
}

fn rejected(response: &TestResponse, message: &str) {
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body.to_string().contains(message),
        "expected „{message}“ in {:?}",
        response.body
    );
}

/// A duplicate name or code is a conflict, not a server fault.
fn conflict(response: &TestResponse, message: &str) {
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    rejected(response, message);
}

fn names(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .map(|value| value["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn heats_are_seeded_with_their_categories_and_finals() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let final_ = json!([{ "id": app.final_id("Finale EW").await, "name": "Finale EW", "code": "Finale EW" }]);
    assert_eq!(
        list_heats(&app).await,
        [
            json!({
                "id": app.heat_id("JE EW").await,
                "name": "JE EW",
                "code": "JE EW",
                "description": "Junior Expert Einzel Weiblich (U11–U15)",
                "categories": ["EWU11", "EWU13", "EWU15"],
                "category_ids": [
                    app.category_id("EWU11").await,
                    app.category_id("EWU13").await,
                    app.category_id("EWU15").await,
                ],
                "act_type": "single",
                "is_single_male": false,
                "finals": final_,
                "threshold": 6,
                "skip": false,
                "act_count": 0,
                "skip_suggested": true,
            }),
            json!({
                "id": app.heat_id("E EW").await,
                "name": "E EW",
                "code": "E EW",
                "description": null,
                "categories": ["EWU17"],
                "category_ids": [app.category_id("EWU17").await],
                "act_type": "single",
                "is_single_male": false,
                "finals": final_,
                "threshold": 5,
                "skip": false,
                "act_count": 0,
                "skip_suggested": true,
            }),
        ]
    );
    // The categories name their heat; clubs never pick it.
    let response = app.get("/api/query/list_categories", None).await;
    let categories = response.body.as_array().unwrap();
    let heat_of =
        |name: &str| categories.iter().find(|c| c["name"] == name).unwrap()["heat"].clone();
    assert_eq!(heat_of("EWU13"), "JE EW");
    let heat_id_of =
        |name: &str| categories.iter().find(|c| c["name"] == name).unwrap()["heat_id"].clone();
    assert_eq!(heat_id_of("EWU13"), json!(app.heat_id("JE EW").await));
    assert_eq!(heat_id_of("EWU19"), Value::Null);
    assert_eq!(heat_of("EWU19"), Value::Null);
}

#[tokio::test]
async fn admins_add_edit_and_delete_heats() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;

    for cookie in [None, Some(owner.as_str())] {
        let response = app
            .post(
                "/api/command/add_heat",
                heat_body("E EM 1", &["EMU17"]),
                cookie,
            )
            .await;
        assert_ne!(response.status, StatusCode::OK);
    }

    let response = app
        .post(
            "/api/command/add_heat",
            heat_body("  E  EM 1 ", &["EMU17", " EMU19", "EMU17"]),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // An empty heat is fine; the admin fills it later.
    let response = app
        .post(
            "/api/command/add_heat",
            heat_body("E EM 2", &[]),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let heats = list_heats(&app).await;
    assert_eq!(names(&heats), ["E EM 1", "JE EW", "E EW", "E EM 2"]);
    assert_eq!(heats[0]["categories"], json!(["EMU17", "EMU19"]));
    assert_eq!(heats[0]["act_type"], "single");
    assert_eq!(heats[0]["is_single_male"], true);
    assert_eq!(heats[0]["finals"], json!([]));
    assert_eq!(heats[3]["categories"], json!([]));
    assert_eq!(heats[3]["act_type"], Value::Null);

    // Renamed with new categories and a description.
    let response = app
        .post(
            "/api/command/edit_heat",
            edit_body(
                app.heat_id("E EM 1").await,
                "Expert 1 EM",
                &["EMU19", "EMU21"],
            ),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let heats = list_heats(&app).await;
    let heat = heats.iter().find(|h| h["name"] == "Expert 1 EM").unwrap();
    assert_eq!(heat["categories"], json!(["EMU19", "EMU21"]));
    assert_eq!(heat["description"], "Expert 1 Einzel Männlich");
    assert!(!names(&heats).contains(&"E EM 1".to_string()));

    // Deleting frees its categories.
    let response = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": app.heat_id("E EM 1").await }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(names(&list_heats(&app).await), ["JE EW", "E EW", "E EM 2"]);
    let response = app
        .post(
            "/api/command/add_heat",
            heat_body("Neu", &["EMU19"]),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": Uuid::new_v4() }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_heat_has_a_readable_name_and_a_unique_code() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let add = |name: &str, code: Value| json!({ "name": name, "code": code, "description": null, "categories": [] });

    let response = app
        .post(
            "/api/command/add_heat",
            add("Einzel männlich – Expert 1 (U17–U21)", json!(" E  EM 1 ")),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // Without a code, the name is the code.
    let response = app
        .post(
            "/api/command/add_heat",
            add("Kürklasse", Value::Null),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let heats = list_heats(&app).await;
    let by_code = |code: &str| heats.iter().find(|h| h["code"] == code).unwrap().clone();
    assert_eq!(
        by_code("E EM 1")["name"],
        "Einzel männlich – Expert 1 (U17–U21)"
    );
    assert_eq!(by_code("Kürklasse")["name"], "Kürklasse");

    // Both are unique.
    let response = app
        .post(
            "/api/command/add_heat",
            add("Anderer Name", json!("E EM 1")),
            Some(&admin),
        )
        .await;
    conflict(&response, "Kürzel „E EM 1“");
    let response = app
        .post(
            "/api/command/add_heat",
            add("Kürklasse", json!("K")),
            Some(&admin),
        )
        .await;
    conflict(&response, "„Kürklasse“ gibt es schon");

    // Edited: the code changes only if given.
    let id = app.heat_id("E EM 1").await;
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": id, "new_name": "Neuer Name", "description": null, "categories": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let heat = list_heats(&app)
        .await
        .into_iter()
        .find(|h| h["id"] == json!(id))
        .unwrap();
    assert_eq!(
        (heat["name"].as_str(), heat["code"].as_str()),
        (Some("Neuer Name"), Some("E EM 1"))
    );
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": id, "new_name": "Neuer Name", "new_code": "EM1", "description": null, "categories": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": id, "new_name": "Neuer Name", "new_code": "E EW", "description": null, "categories": [] }),
            Some(&admin),
        )
        .await;
    conflict(&response, "Kürzel „E EW“");
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": id, "new_name": "Kürklasse", "description": null, "categories": [] }),
            Some(&admin),
        )
        .await;
    conflict(&response, "„Kürklasse“ gibt es schon");
}

#[tokio::test]
async fn a_heat_keeps_to_one_discipline_and_its_categories_to_one_heat() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    for (body, message) in [
        (heat_body(" ", &[]), "Namen"),
        (heat_body("JE EW", &[]), "gibt es schon"),
        (heat_body("X", &["EMU99"]), "gibt es nicht"),
        // One act type and gender.
        (heat_body("X", &["EMU17", "EWU19"]), "EWU19"),
        (heat_body("X", &["PU17", "EWU19"]), "EWU19"),
        // A category belongs to at most one heat.
        (heat_body("X", &["EWU19", "EWU15"]), "„JE EW“"),
    ] {
        let response = app.post("/api/command/add_heat", body, Some(&admin)).await;
        rejected(&response, message);
    }
    assert_eq!(names(&list_heats(&app).await), ["JE EW", "E EW"]);

    // A heat in a final keeps the final's discipline.
    let response = app
        .post(
            "/api/command/edit_heat",
            edit_body(app.heat_id("E EW").await, "E EW", &["EMU17"]),
            Some(&admin),
        )
        .await;
    rejected(&response, "Finale EW");
    // Moving a category to another heat takes two steps.
    let response = app
        .post(
            "/api/command/edit_heat",
            edit_body(app.heat_id("E EW").await, "E EW", &["EWU15", "EWU17"]),
            Some(&admin),
        )
        .await;
    rejected(&response, "„JE EW“");
}

#[tokio::test]
async fn a_heat_in_a_final_is_renamed_with_it_but_not_deleted() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": app.heat_id("E EW").await }),
            Some(&admin),
        )
        .await;
    rejected(&response, "Finale EW");
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);

    let response = app
        .post(
            "/api/command/edit_heat",
            edit_body(app.heat_id("E EW").await, "Expert EW", &["EWU17", "EWU19"]),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app.get("/api/query/list_finals", None).await;
    let final_ = &response.body[0];
    assert_eq!(
        final_["slot_groups"],
        json!([
            { "heat_id": app.heat_id("JE EW").await, "heat": "JE EW", "heat_code": "JE EW", "count": 2 },
            { "heat_id": app.heat_id("E EW").await, "heat": "Expert EW", "heat_code": "E EW", "count": 1 },
        ])
    );
    assert_eq!(final_["slots"][2]["label"], "1. Finalist (E EW)");
    assert_eq!(
        final_["categories"],
        json!(["EWU11", "EWU13", "EWU15", "EWU17", "EWU19"])
    );
}

#[tokio::test]
async fn a_category_in_a_heat_keeps_its_discipline() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let category = |name: &str, act_type: &str, male: bool| {
        json!({
            "name": name,
            "new_name": name,
            "description": "Einzel Weiblich U17",
            "min_age": 15,
            "under_age": 17,
            "act_type": act_type,
            "is_single_male": male,
            "einfahrzeit_seconds": 600,
            "act_duration_seconds": 180,
            "judge_duration_seconds": 360,
        })
    };
    for body in [
        category("EWU17", "single", true),
        category("EWU17", "pair", false),
    ] {
        let response = app
            .post("/api/command/edit_category", body, Some(&admin))
            .await;
        rejected(&response, "„E EW“");
    }
    // Other changes are fine, and a category outside a heat changes freely.
    let response = app
        .post(
            "/api/command/edit_category",
            category("EWU17", "single", false),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/edit_category",
            category("EWU19", "single", true),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn the_import_preview_names_changed_heats() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let file = include_str!("fixtures/competition.toml").replacen(
        "categories = [\"EWU17\"]",
        "categories = [\"EWU17\", \"EWU19\"]",
        1,
    );
    let response = app
        .post(
            "/api/command/preview_competition_import",
            json!({ "file": file }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // The test app's settings differ from the file; the categories and
    // finals don't.
    let changes = response.body["changes"].as_array().unwrap();
    assert!(changes.contains(&json!("heats")), "{changes:?}");
    assert!(!changes.contains(&json!("categories")), "{changes:?}");
    assert!(!changes.contains(&json!("finals")), "{changes:?}");
}
