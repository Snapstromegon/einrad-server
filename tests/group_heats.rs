//! Integration tests for group heats and finals (#345): every category
//! belongs to a heat, groups start in their final, their Vorlauf skipped.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use serde_json::{Value, json};

fn ok(response: &TestResponse) {
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

async fn heat(app: &TestApp, name: &str) -> Value {
    let response = app.get("/api/query/list_heats", None).await;
    ok(&response);
    response
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|heat| heat["name"] == name)
        .unwrap_or_else(|| panic!("no heat {name}"))
        .clone()
}

fn heat_body(name: &str, categories: &[&str]) -> Value {
    json!({ "name": name, "description": null, "categories": categories })
}

fn final_body(name: &str, act_type: &str, heat: &str) -> Value {
    json!({
        "name": name,
        "description": null,
        "act_type": act_type,
        "is_single_male": false,
        "einfahrzeit_seconds": 600,
        "act_duration_seconds": 300,
        "judge_duration_seconds": 180,
        "slot_groups": [{ "heat": heat, "count": 1 }],
    })
}

#[tokio::test]
async fn group_categories_form_heats_with_the_vorlauf_skipped() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    ok(&app
        .post(
            "/api/command/add_heat",
            heat_body("Kleingruppe Expert", &["KG15+"]),
            Some(&admin),
        )
        .await);
    let group = heat(&app, "Kleingruppe Expert").await;
    assert_eq!(group["skip"], true);
    // The count doesn't matter: nothing is suggested either way.
    assert_eq!(group["skip_suggested"], false);
    assert_eq!(group["act_type"], "group_small");

    // Singles keep their Vorlauf; the organizer may un-skip a group's.
    ok(&app
        .post(
            "/api/command/add_heat",
            heat_body("Einzel", &["EMU70"]),
            Some(&admin),
        )
        .await);
    assert_eq!(heat(&app, "Einzel").await["skip"], false);
    ok(&app
        .post(
            "/api/command/set_heat_skip",
            json!({ "name": "Kleingruppe Expert", "skip": false }),
            Some(&admin),
        )
        .await);
    assert_eq!(heat(&app, "Kleingruppe Expert").await["skip"], false);

    // Heats share one discipline: Kleingruppe is not Großgruppe.
    rejected(
        &app.post(
            "/api/command/add_heat",
            heat_body("Gemischt", &["KGU15", "GGU15"]),
            Some(&admin),
        )
        .await,
        "GGU15",
    );
}

#[tokio::test]
async fn a_group_final_is_filled_from_group_heats() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    ok(&app
        .post(
            "/api/command/add_heat",
            heat_body("Kleingruppe Expert", &["KG15+"]),
            Some(&admin),
        )
        .await);
    ok(&app
        .post(
            "/api/command/add_final",
            final_body("Finale KG Expert", "group_small", "Kleingruppe Expert"),
            Some(&admin),
        )
        .await);
    // Of another discipline than its heat.
    rejected(
        &app.post(
            "/api/command/add_final",
            final_body("Finale GG", "group_large", "Kleingruppe Expert"),
            Some(&admin),
        )
        .await,
        "Kleingruppe Expert",
    );
}

// The migration.

/// The last migration before groups got heats.
const BEFORE_GROUP_HEATS: i64 = 20261113120000;

async fn exec(db: &sqlx::SqlitePool, sql: &'static str) {
    sqlx::query(sql).execute(db).await.unwrap();
}

async fn texts(db: &sqlx::SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

#[tokio::test]
async fn the_migration_gives_group_categories_a_heat_and_a_final() {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE_GROUP_HEATS, &db).await.unwrap();
    exec(
        &db,
        "INSERT INTO categories (name, act_type, is_single_male, under_age, \"order\", \
         einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds) VALUES \
         ('EMU17', 'single', TRUE, 17, 1, 300, 180, 120), \
         ('KG', 'group_small', FALSE, 15, 2, 600, 300, 180), \
         ('GG', 'group_large', FALSE, NULL, 3, 600, 360, 180)",
    )
    .await;
    // The names are taken: "KG" by a heat and "KG 2" too, "GG" by a final.
    exec(&db, "INSERT INTO heats (name) VALUES ('KG'), ('KG 2')").await;
    exec(
        &db,
        "INSERT INTO finals (name, act_type, \"order\") VALUES ('GG', 'single', 1)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO timeplan (id, kind, category, position, earliest_start_time) VALUES \
         (1, 'acts', 'EMU17', 1, NULL), \
         (2, 'acts', 'KG', 2, '2027-06-05T10:00:00Z'), \
         (3, 'custom', NULL, 3, NULL), \
         (4, 'acts', 'GG', 4, NULL)",
    )
    .await;
    exec(
        &db,
        "UPDATE timeplan SET label = 'Pause', duration_seconds = 600 WHERE id = 3",
    )
    .await;
    exec(
        &db,
        "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) \
         VALUES (2, 0, 'Einfahren', 300)",
    )
    .await;

    migrator.run(&db).await.unwrap();

    // Heats named after the category, " 2", " 3" added where taken.
    assert_eq!(
        texts(
            &db,
            "SELECT name || ':' || threshold || ':' || skip FROM heats ORDER BY name"
        )
        .await,
        ["GG 2:5:1", "KG:5:0", "KG 2:5:0", "KG 3:6:1"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT name || '>' || heat FROM categories WHERE heat IS NOT NULL ORDER BY name"
        )
        .await,
        ["GG>GG 2", "KG>KG 3"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT name || ':' || act_type || ':' || \"order\" || ':' || einfahrzeit_seconds \
             || ':' || act_duration_seconds || ':' || judge_duration_seconds \
             FROM finals WHERE name != 'GG' ORDER BY \"order\""
        )
        .await,
        [
            "KG 3:group_small:2:600:300:180",
            "GG 2:group_large:3:600:360:180"
        ]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT final_name || ':' || heat || ':' || number FROM final_slots ORDER BY final_name"
        )
        .await,
        ["GG 2:GG 2:1", "KG 3:KG 3:1"]
    );
    // The category blocks stay; the final's block follows its category's,
    // with its start time and its stored slots.
    assert_eq!(
        texts(
            &db,
            "SELECT COALESCE(category, final_name, label) FROM timeplan ORDER BY position"
        )
        .await,
        ["EMU17", "KG", "KG 3", "Pause", "GG", "GG 2"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT earliest_start_time FROM timeplan WHERE final_name = 'KG 3'"
        )
        .await,
        ["2027-06-05T10:00:00Z"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT timeplan.final_name FROM timeplan_slots \
             JOIN timeplan ON timeplan.id = timeplan_id"
        )
        .await,
        ["KG 3"]
    );

    migrator.undo(&db, BEFORE_GROUP_HEATS).await.unwrap();
    assert_eq!(
        texts(&db, "SELECT name FROM heats ORDER BY name").await,
        ["KG", "KG 2"]
    );
    assert_eq!(texts(&db, "SELECT name FROM finals").await, ["GG"]);
    assert_eq!(
        texts(
            &db,
            "SELECT COALESCE(category, label) FROM timeplan ORDER BY position"
        )
        .await,
        ["EMU17", "KG", "Pause", "GG"]
    );
    assert_eq!(
        texts(&db, "SELECT timeplan.category FROM timeplan_slots JOIN timeplan ON timeplan.id = timeplan_id")
            .await,
        Vec::<String>::new()
            .into_iter()
            .chain(["KG".to_string()])
            .collect::<Vec<_>>()
    );
}
