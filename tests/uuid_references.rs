//! References to heats and finals by UUID (#347): the migration from name
//! references, and what renaming does and does not touch.

mod common;

const BEFORE_UUID_REFERENCES: i64 = 20261116120000;

async fn exec(db: &sqlx::SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn texts(db: &sqlx::SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

/// A database before the migration with two heats ("E EM" fills the final,
/// "KG" is a group heat without a final), one final in the timeplan, a
/// finalist slot in its block and a judge registered for the heat.
async fn before() -> (sqlx::SqlitePool, sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE_UUID_REFERENCES, &db).await.unwrap();
    exec(&db, "INSERT INTO heats (name) VALUES ('E EM'), ('KG')").await;
    exec(
        &db,
        "INSERT INTO finals (name, act_type, \"order\") VALUES ('Finale EM', 'single', 1)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO categories (name, act_type, is_single_male, heat) VALUES \
         ('EMU17', 'single', TRUE, 'E EM'), ('EMU19', 'single', TRUE, 'E EM'), \
         ('KG1', 'group_small', FALSE, 'KG'), ('Ohne', 'pair', FALSE, NULL)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO final_slots (id, final_name, position, number, heat) VALUES \
         (x'a1', 'Finale EM', 1, 1, 'E EM'), (x'a2', 'Finale EM', 2, 2, 'E EM')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO timeplan (id, kind, final_name, position) VALUES (1, 'acts', 'Finale EM', 1)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO timeplan_slots (timeplan_id, position, heat, number) VALUES (1, 1, 'E EM', 2)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO users (id, email, email_verified, name, password) \
         VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) \
         VALUES (x'20', x'10', 'Max', 'Müller', '1990-01-01T00:00:00Z', TRUE)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO judge_registration (judge_id, heat_id, task) \
         SELECT x'20', id, 'technical' FROM heats WHERE name = 'E EM'",
    )
    .await;
    (db, migrator)
}

#[tokio::test]
async fn the_migration_turns_name_references_into_ids() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        texts(
            &db,
            "SELECT heats.name || ' ' || categories.name FROM categories \
             JOIN heats ON heats.id = categories.heat_id ORDER BY categories.name"
        )
        .await,
        ["E EM EMU17", "E EM EMU19", "KG KG1"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM categories WHERE name = 'Ohne' AND heat_id IS NULL"
        )
        .await,
        ["1"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT finals.name || ' ' || heats.name || ' ' || final_slots.number \
             FROM final_slots \
             JOIN finals ON finals.id = final_slots.final_id \
             JOIN heats ON heats.id = final_slots.heat_id ORDER BY position"
        )
        .await,
        ["Finale EM E EM 1", "Finale EM E EM 2"]
    );
    // The slots get valid v7 ids in the later re-mint (#355), one each.
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(DISTINCT id) AS TEXT) FROM final_slots WHERE length(id) = 16"
        )
        .await,
        ["2"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT finals.name FROM timeplan JOIN finals ON finals.id = timeplan.final_id"
        )
        .await,
        ["Finale EM"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT heats.name || ' ' || timeplan_slots.number FROM timeplan_slots \
             JOIN heats ON heats.id = timeplan_slots.heat_id"
        )
        .await,
        ["E EM 2"]
    );
    // The jury registration was by id before and stays.
    assert_eq!(
        texts(
            &db,
            "SELECT heats.name FROM judge_registration JOIN heats ON heats.id = heat_id"
        )
        .await,
        ["E EM"]
    );
}

#[tokio::test]
async fn renaming_a_heat_or_final_is_one_update_and_keeps_every_reference() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    exec(
        &db,
        "UPDATE heats SET name = 'Einzel Expert' WHERE name = 'E EM'",
    )
    .await;
    exec(
        &db,
        "UPDATE finals SET name = 'Finale Expert' WHERE name = 'Finale EM'",
    )
    .await;

    assert_eq!(
        texts(
            &db,
            "SELECT heats.name FROM categories JOIN heats ON heats.id = categories.heat_id \
             WHERE categories.name = 'EMU17'"
        )
        .await,
        ["Einzel Expert"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT finals.name || ' ' || heats.name FROM final_slots \
             JOIN finals ON finals.id = final_id JOIN heats ON heats.id = heat_id LIMIT 1"
        )
        .await,
        ["Finale Expert Einzel Expert"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT finals.name FROM timeplan JOIN finals ON finals.id = final_id"
        )
        .await,
        ["Finale Expert"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT heats.name FROM timeplan_slots JOIN heats ON heats.id = heat_id"
        )
        .await,
        ["Einzel Expert"]
    );
}

#[tokio::test]
async fn deleting_keeps_the_on_delete_behaviour_of_the_name_references() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    // A heat that fills slots can't be deleted.
    assert!(
        sqlx::query("DELETE FROM heats WHERE name = 'E EM'")
            .execute(&db)
            .await
            .is_err()
    );
    // Deleting a final takes its slots and its block along.
    exec(&db, "DELETE FROM finals WHERE name = 'Finale EM'").await;
    assert_eq!(
        texts(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM final_slots").await,
        ["0"]
    );
    assert_eq!(
        texts(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM timeplan_slots").await,
        ["0"]
    );
    assert_eq!(
        texts(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM timeplan").await,
        ["0"]
    );
    // Now the heat can go (a judge registered for it would hold it): its
    // categories stay, without a heat.
    exec(&db, "DELETE FROM judge_registration").await;
    exec(&db, "DELETE FROM heats WHERE name = 'E EM'").await;
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM categories WHERE heat_id IS NULL"
        )
        .await,
        ["3"]
    );
}

#[tokio::test]
async fn the_down_migration_writes_the_names_back() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    exec(
        &db,
        "UPDATE heats SET name = 'Einzel Expert' WHERE name = 'E EM'",
    )
    .await;
    migrator.undo(&db, BEFORE_UUID_REFERENCES).await.unwrap();

    assert_eq!(
        texts(
            &db,
            "SELECT name || ' ' || heat FROM categories WHERE heat IS NOT NULL ORDER BY name"
        )
        .await,
        // Back to names, which are the codes (ADR 0017): the rename of the
        // readable name is not what the old references used.
        ["EMU17 E EM", "EMU19 E EM", "KG1 KG"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT final_name || ' ' || heat || ' ' || number FROM final_slots ORDER BY position"
        )
        .await,
        ["Finale EM E EM 1", "Finale EM E EM 2"]
    );
    assert_eq!(
        texts(&db, "SELECT final_name FROM timeplan").await,
        ["Finale EM"]
    );
    assert_eq!(
        texts(&db, "SELECT heat || ' ' || number FROM timeplan_slots").await,
        ["E EM 2"]
    );
    // And up again.
    migrator.run(&db).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT heats.name FROM timeplan_slots JOIN heats ON heats.id = heat_id"
        )
        .await,
        ["Einzel Expert"]
    );
}

// ---------- Over the API ----------

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{Value, json};
use uuid::Uuid;

/// Born so that the judge is 30 on the first competition day.
const ADULT: &str = "1997-01-01T00:00:00.000Z";

async fn get(app: &TestApp, path: &str, cookie: Option<&str>) -> Value {
    let response = app.get(path, cookie).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

async fn post(app: &TestApp, cookie: &str, command: &str, body: Value) -> common::TestResponse {
    app.post(&format!("/api/command/{command}"), body, Some(cookie))
        .await
}

/// The entry of the list (`list_heats`, `list_finals`) named `name`.
async fn named(app: &TestApp, query: &str, name: &str) -> Value {
    get(app, &format!("/api/query/{query}"), None)
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {query}"))
        .clone()
}

async fn jury_unit(app: &TestApp, admin: &str, name: &str) -> Value {
    get(app, "/api/query/list_jury_units", Some(admin))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["name"] == name)
        .unwrap_or_else(|| panic!("no jury unit {name}"))
        .clone()
}

fn registration(unit: &Value, task: &str) -> Value {
    json!({ "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task })
}

/// A start block for the final "Finale EW" with a finalist slot of "E EW"
/// stored in it, and a judge registered for both.
#[tokio::test]
async fn renaming_a_heat_and_a_final_keeps_every_reference() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = named(&app, "list_heats", "E EW").await;
    let final_ = named(&app, "list_finals", "Finale EW").await;
    let heat_id = heat["id"].clone();
    let final_id = final_["id"].clone();
    assert!(Uuid::parse_str(heat_id.as_str().unwrap()).is_ok());

    // The final runs in a block, with a finalist slot of the heat stored.
    let response = post(
        &app,
        &admin,
        "add_timeplan_entry",
        json!({ "final_id": final_id, "label": null, "category": null, "duration_seconds": null,
                "earliest_start_time": null }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let block = get(&app, "/api/query/list_timeplan", None)
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["final_id"] == final_id)
        .unwrap()
        .clone();
    let response = post(
        &app,
        &admin,
        "set_block_order",
        json!({ "timeplan_id": block["id"], "slots": [{ "Finalist": { "heat_id": heat_id, "number": 1 } }] }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // A judge is registered for the heat and the final.
    let response = post(
        &app,
        &owner,
        "add_club_judge",
        json!({
            "club_id": club_id, "firstname": "Anna", "lastname": "Jury", "birthdate": ADULT,
            "is_trained": true, "max_deployments": null, "note": null,
            "registrations": [
                registration(&jury_unit(&app, &admin, "E EW").await, "technical"),
                registration(&jury_unit(&app, &admin, "Finale EW").await, "performance"),
            ],
        }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // Renaming is a plain update of the names.
    let response = post(
        &app,
        &admin,
        "edit_heat",
        json!({ "id": heat_id, "new_name": "Expert EW", "description": null, "categories": ["EWU17"] }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = post(
        &app,
        &admin,
        "edit_final",
        json!({
            "id": final_id, "new_name": "Finale Expert", "description": "Finale Einzel Weiblich",
            "act_type": "single", "is_single_male": false, "einfahrzeit_seconds": 300,
            "act_duration_seconds": 180, "judge_duration_seconds": 120,
            "slot_groups": [
                { "heat_id": named(&app, "list_heats", "JE EW").await["id"], "count": 2 },
                { "heat_id": heat_id, "count": 1 },
            ],
        }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // The ids are the same, the names are the new ones.
    let heat = named(&app, "list_heats", "Expert EW").await;
    assert_eq!(heat["id"], heat_id);
    assert_eq!(heat["categories"], json!(["EWU17"]));
    assert_eq!(
        heat["finals"],
        json!([{ "id": final_id, "name": "Finale Expert", "code": "Finale EW" }])
    );
    let final_ = named(&app, "list_finals", "Finale Expert").await;
    assert_eq!(final_["id"], final_id);
    assert_eq!(final_["slot_groups"][1]["heat_id"], heat_id);
    assert_eq!(final_["slot_groups"][1]["heat"], "Expert EW");
    assert_eq!(final_["slots"][2]["label"], "1. Finalist (E EW)");
    let categories = get(&app, "/api/query/list_categories", None).await;
    let ewu17 = categories
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "EWU17")
        .unwrap();
    assert_eq!(ewu17["heat_id"], heat_id);
    assert_eq!(ewu17["heat"], "Expert EW");
    let block = get(&app, "/api/query/list_timeplan", None)
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["final_id"] == final_id)
        .unwrap()
        .clone();
    assert_eq!(block["final_name"], "Finale Expert");
    // The stored finalist slot of the block still names the heat.
    let predicted = get(&app, "/api/query/predict_timeplan", None).await;
    let slots = predicted["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["timeplan_entry"]["Acts"]["final_id"] == final_id)
        .unwrap()["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(slots[0]["Finalist"]["heat_id"], heat_id);
    assert_eq!(slots[0]["Finalist"]["group_label"], "E EW");
    // The registrations.
    assert_eq!(jury_unit(&app, &admin, "Expert EW").await["id"], heat_id);
    assert_eq!(
        jury_unit(&app, &admin, "Finale Expert").await["id"],
        final_id
    );
    let registrations: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE((SELECT name FROM heats WHERE id = heat_id), \
            (SELECT name FROM finals WHERE id = final_id)) FROM judge_registration ORDER BY 1",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(registrations, ["Expert EW", "Finale Expert"]);

    // The export writes the new names, the codes the references use, and
    // no ids.
    let file = get(&app, "/api/query/export_competition", Some(&admin))
        .await
        .as_str()
        .unwrap()
        .to_string();
    assert!(file.contains("name = \"Expert EW\""), "{file}");
    assert!(file.contains("name = \"Finale Expert\""), "{file}");
    assert!(file.contains("code = \"E EW\""), "{file}");
    assert!(file.contains("heat = \"E EW\""), "{file}");
    assert!(file.contains("final = \"Finale EW\""), "{file}");
}

/// The competition file names heats and finals; the import resolves the
/// names to ids and the export writes the names again.
#[tokio::test]
async fn the_file_keeps_names_and_the_database_ids() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let file = include_str!("fixtures/competition.toml").replacen(
        "[[timeplan]]\ncategory = \"EMU11\"",
        "[[timeplan]]\nfinal = \"Finale EW\"\n\n[[timeplan]]\ncategory = \"EMU11\"",
        1,
    );
    let response = post(&app, &admin, "import_competition", json!({ "file": file })).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // In the database every reference is the id of the heat or final.
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT finals.name || '>' || heats.name || '>' || final_slots.number FROM final_slots \
         JOIN finals ON finals.id = final_slots.final_id \
         JOIN heats ON heats.id = final_slots.heat_id ORDER BY position",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        rows,
        ["Finale EW>JE EW>1", "Finale EW>JE EW>2", "Finale EW>E EW>1"]
    );
    let planned: Vec<String> = sqlx::query_scalar(
        "SELECT finals.name FROM timeplan JOIN finals ON finals.id = timeplan.final_id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(planned, ["Finale EW"]);
    let categories: Vec<String> = sqlx::query_scalar(
        "SELECT heats.name FROM categories JOIN heats ON heats.id = categories.heat_id \
         WHERE categories.name = 'EWU17'",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(categories, ["E EW"]);

    // The export is the file again: names, no ids.
    let exported = get(&app, "/api/query/export_competition", Some(&admin))
        .await
        .as_str()
        .unwrap()
        .to_string();
    assert!(exported.contains("final = \"Finale EW\""), "{exported}");
    assert!(
        exported.contains("[[finals.slots]]\nheat = \"JE EW\"\ncount = 2\n"),
        "{exported}"
    );
    assert!(
        exported.contains("[[finals.slots]]\nheat = \"E EW\"\ncount = 1\n"),
        "{exported}"
    );
    assert!(exported.contains("categories = [\"EWU17\"]"), "{exported}");
    let has_uuid = exported
        .split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .any(|word| Uuid::parse_str(word).is_ok());
    assert!(!has_uuid, "{exported}");
}
