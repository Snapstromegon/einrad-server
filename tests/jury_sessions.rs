//! Jury sessions (Jurydurchgänge, #386, ADR 0019): the migration gives every
//! heat and final one session, the default session of new units, and the
//! orphan rule for categories that join later.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::{
    ids::{CategoryId, ClubId, JurySessionId, JuryUnitId},
    jury::JuryUnitKind,
    jury_sessions,
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

const BEFORE: i64 = 20261121120000;

const HEAT_EM: &str = "x'aa000000000000000000000000000001'";
const HEAT_KG: &str = "x'bb000000000000000000000000000002'";
const FINAL_EM: &str = "x'cc000000000000000000000000000003'";

async fn exec(db: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn texts(db: &SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

async fn count(db: &SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.unwrap()
}

fn is_v7(id: &[u8]) -> bool {
    id.len() == 16 && id[6] >> 4 == 7 && id[8] >> 6 == 0b10
}

/// A database just before the migration: heats E EM (two categories) and KG,
/// the final Finale EM fed by E EM, and judges Max Müller (technical in the
/// heat) and Erika Mustermann (performance in the final).
async fn before() -> (SqlitePool, sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE, &db).await.unwrap();
    for sql in [
        format!("INSERT INTO heats (id, name, code) VALUES ({HEAT_EM}, 'Einzel EM', 'E EM'), ({HEAT_KG}, 'Kleine Gruppen', 'KG')"),
        format!("INSERT INTO finals (id, name, code, act_type, \"order\") VALUES ({FINAL_EM}, 'Finale EM', 'F EM', 'single', 1)"),
        format!("INSERT INTO categories (id, name, act_type, is_single_male, heat_id) VALUES \
         (x'01010101010101010101010101010101', 'EMU17', 'single', TRUE, {HEAT_EM}), \
         (x'02020202020202020202020202020202', 'EMU23', 'single', TRUE, {HEAT_EM}), \
         (x'03030303030303030303030303030303', 'KG1', 'group_small', FALSE, {HEAT_KG}), \
         (x'04040404040404040404040404040404', 'Ohne', 'pair', FALSE, NULL)"),
        format!("INSERT INTO final_slots (id, final_id, position, number, heat_id) VALUES \
         (x'a1', {FINAL_EM}, 1, 1, {HEAT_EM})"),
        "INSERT INTO users (id, email, email_verified, name, password) VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')".to_string(),
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')".to_string(),
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES \
         (x'20', x'10', 'Max', 'Müller', '1990-01-01T00:00:00Z', TRUE), \
         (x'21', x'10', 'Erika', 'Mustermann', '1991-01-01T00:00:00Z', TRUE)".to_string(),
        format!("INSERT INTO jury_assignments (judge_id, heat_id, task) VALUES (x'20', {HEAT_EM}, 'technical')"),
        format!("INSERT INTO jury_assignments (judge_id, final_id, task) VALUES (x'21', {FINAL_EM}, 'performance')"),
    ] {
        exec(&db, &sql).await;
    }
    (db, migrator)
}

#[tokio::test]
async fn the_migration_gives_every_unit_one_session_and_keeps_assignments() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(count(&db, "SELECT COUNT(*) FROM jury_sessions").await, 3);
    for table in ["heats", "finals"] {
        let sql: &'static str = if table == "heats" {
            "SELECT COUNT(*) FROM heats h WHERE (SELECT COUNT(*) FROM jury_sessions s WHERE s.unit_id = h.id) = 1"
        } else {
            "SELECT COUNT(*) FROM finals f WHERE (SELECT COUNT(*) FROM jury_sessions s WHERE s.unit_id = f.id) = 1"
        };
        let expected = if table == "heats" { 2 } else { 1 };
        assert_eq!(count(&db, sql).await, expected, "{table}");
    }
    let sessions: Vec<Vec<u8>> = sqlx::query_scalar("SELECT id FROM jury_sessions")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(sessions.iter().all(|id| is_v7(id)), "{sessions:?}");

    let assignments: Vec<Vec<u8>> = sqlx::query_scalar("SELECT id FROM jury_assignments")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(assignments.len(), 2);
    assert!(assignments.iter().all(|id| is_v7(id)), "{assignments:?}");
    assert_ne!(assignments[0], assignments[1]);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM jury_assignments a JOIN jury_sessions s ON s.id = a.session_id \
             WHERE s.unit_id = a.unit_id"
        )
        .await,
        2
    );

    // Every category of a heat, and of the heats feeding a final, is in the
    // unit's session.
    assert_eq!(
        texts(
            &db,
            "SELECT COALESCE(h.name, f.name) || ' ' || c.name FROM jury_session_categories jc \
             JOIN jury_sessions s ON s.id = jc.session_id \
             JOIN categories c ON c.id = jc.category_id \
             LEFT JOIN heats h ON h.id = s.heat_id \
             LEFT JOIN finals f ON f.id = s.final_id ORDER BY 1"
        )
        .await,
        [
            "Einzel EM EMU17",
            "Einzel EM EMU23",
            "Finale EM EMU17",
            "Finale EM EMU23",
            "Kleine Gruppen KG1"
        ]
    );

    // A judge sits once per session, but may sit in several.
    let duplicate = "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task) \
         SELECT x'20', heat_id, id, 'dismounts' FROM jury_sessions WHERE heat_id = ";
    let heat_session = format!("{duplicate}{HEAT_EM}");
    assert!(
        sqlx::query(sqlx::AssertSqlSafe(heat_session))
            .execute(&db)
            .await
            .is_err()
    );
    exec(
        &db,
        &format!(
            "INSERT INTO jury_sessions (id, heat_id) VALUES (x'0190f2d4b1c07abc8def0123456789ab', {HEAT_EM})"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task) \
             VALUES (x'20', {HEAT_EM}, x'0190f2d4b1c07abc8def0123456789ab', 'dismounts')"
        ),
    )
    .await;
}

#[tokio::test]
async fn the_down_migration_merges_sessions() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let first: Vec<u8> = sqlx::query_scalar(
        "SELECT id FROM jury_sessions WHERE heat_id = x'aa000000000000000000000000000001'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(is_v7(&first));
    // A younger second session, with Max in it as well.
    exec(
        &db,
        &format!(
            "INSERT INTO jury_sessions (id, heat_id) VALUES (x'ffffffffffff7fffbfffffffffffffff', {HEAT_EM})"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task) \
             VALUES (x'20', {HEAT_EM}, x'ffffffffffff7fffbfffffffffffffff', 'performance')"
        ),
    )
    .await;

    migrator.undo(&db, BEFORE).await.unwrap();

    assert_eq!(
        texts(
            &db,
            "SELECT task FROM jury_assignments WHERE judge_id = x'20'"
        )
        .await,
        ["technical"]
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM jury_assignments").await, 2);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'jury_session%'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'jury_assignments_unique' \
             AND sql LIKE '%judge_id%unit_id%'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn orphan_categories_belong_to_the_first_session() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let mut conn = db.acquire().await.unwrap();
    let heat = JuryUnitId(Uuid::parse_str("aa000000-0000-0000-0000-000000000001").unwrap());
    let first = jury_sessions::first_session(&mut conn, heat)
        .await
        .unwrap()
        .unwrap();
    // A second session that holds EMU23 only.
    let second = JurySessionId::new();
    sqlx::query("INSERT INTO jury_sessions (id, heat_id) VALUES (?, ?)")
        .bind(second)
        .bind(heat)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE jury_session_categories SET session_id = ? WHERE unit_id = ? AND category_id = x'02020202020202020202020202020202'",
    )
    .bind(second)
    .bind(heat)
    .execute(&mut *conn)
    .await
    .unwrap();
    // A category joins the heat after the split; one with a row leaves it.
    sqlx::query("INSERT INTO categories (id, name, act_type, is_single_male, heat_id) VALUES (x'05050505050505050505050505050505', 'EMU30', 'single', TRUE, ?)")
        .bind(heat)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE categories SET heat_id = NULL WHERE id = x'01010101010101010101010101010101'",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let map = jury_sessions::session_of_category(&mut conn, JuryUnitKind::Heat, heat)
        .await
        .unwrap();
    let cat = |n: u8| CategoryId(Uuid::from_bytes([n; 16]));
    assert_eq!(map.len(), 2, "{map:?}");
    assert_eq!(map[&cat(2)], second);
    assert_eq!(map[&cat(5)], first, "the orphan joins the first session");
    assert!(!map.contains_key(&cat(1)), "a category that left is stale");
}

// ---------- Over the API ----------

async fn sessions(app: &TestApp) -> i64 {
    count(&app.db, "SELECT COUNT(*) FROM jury_sessions").await
}

fn heat_body(name: &str, category_ids: &[CategoryId]) -> Value {
    json!({ "name": name, "description": null, "category_ids": category_ids })
}

#[tokio::test]
async fn adding_and_deleting_units_creates_and_removes_their_sessions() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let before = sessions(&app).await;

    let response = app
        .post(
            "/api/command/add_heat",
            heat_body("Neu", &[app.category_id("EMU19").await]),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(sessions(&app).await, before + 1);
    let heat = app.heat_id("Neu").await;
    // The new session inherits the unit's needed counts.
    assert_eq!(
        count(
            &app.db,
            "SELECT COUNT(*) FROM jury_sessions WHERE needed_technical IS NOT NULL \
             OR needed_performance IS NOT NULL OR needed_dismounts IS NOT NULL"
        )
        .await,
        0
    );

    let response = app
        .post(
            "/api/command/add_final",
            json!({
                "name": "Finale Neu", "description": null, "act_type": "single",
                "is_single_male": true, "einfahrzeit_seconds": 600,
                "act_duration_seconds": 300, "judge_duration_seconds": 180,
                "slot_groups": [{ "heat_id": heat, "count": 1 }],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(sessions(&app).await, before + 2);
    let final_id = app.final_id("Finale Neu").await;

    for (command, id) in [("delete_final", final_id.0), ("delete_heat", heat.0)] {
        let response = app
            .post(
                &format!("/api/command/{command}"),
                json!({ "id": id }),
                Some(&admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
    assert_eq!(sessions(&app).await, before);
}

#[tokio::test]
async fn a_competition_file_replace_leaves_one_session_per_unit() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let units = count(
        &app.db,
        "SELECT (SELECT COUNT(*) FROM heats) + (SELECT COUNT(*) FROM finals)",
    )
    .await;
    assert_eq!(sessions(&app).await, units);

    let file = include_str!("fixtures/competition.toml").replacen(
        "fee_per_start = 15",
        "fee_per_start = 20",
        1,
    );
    let response = app
        .post(
            "/api/command/import_competition",
            json!({ "file": file }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "replaced");

    assert_eq!(sessions(&app).await, units);
    assert_eq!(
        count(
            &app.db,
            "SELECT COUNT(*) FROM (SELECT unit_id FROM jury_sessions GROUP BY unit_id HAVING COUNT(*) = 1)"
        )
        .await,
        units
    );
    assert_eq!(
        count(
            &app.db,
            "SELECT COUNT(*) FROM jury_sessions WHERE unit_id NOT IN \
             (SELECT id FROM heats UNION SELECT id FROM finals)"
        )
        .await,
        0
    );
    assert_eq!(
        count(&app.db, "SELECT COUNT(*) FROM jury_session_categories").await,
        0
    );
}

// ---------- The sessions API (Task 2) ----------

const DUPLICATE_JUDGE: &str = "Eine JurorIn kann in einem Durchgang nur eine Aufgabe haben: Max Müller wäre mehrfach eingeteilt.";
const FOREIGN_ASSIGNMENT: &str =
    "Diese Einteilung gehört nicht zu einem zusammengelegten Durchgang.";
const EVERY_CATEGORY_ONCE: &str = "Jede Kategorie gehört in genau einen Durchgang.";
const NEEDS_A_CATEGORY: &str = "Ein Durchgang braucht mindestens eine Kategorie.";
const ALREADY_ASSIGNED: &str = "Die JurorIn ist hier schon eingeteilt.";

/// The German text of an `ErrorMessages` refusal (the API answers 500 with
/// the message Debug-quoted).
fn message(response: &common::TestResponse) -> String {
    response.body["message"]
        .as_str()
        .unwrap()
        .trim_matches('"')
        .to_string()
}

async fn units(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn unit_named(app: &TestApp, cookie: &str, name: &str) -> Value {
    units(app, cookie)
        .await
        .into_iter()
        .find(|u| u["name"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
}

/// Adds a heat with these categories and returns its jury unit.
async fn new_heat(app: &TestApp, admin: &str, name: &str, categories: &[&str]) -> Value {
    let mut ids = vec![];
    for category in categories {
        ids.push(app.category_id(category).await);
    }
    let response = app
        .post("/api/command/add_heat", heat_body(name, &ids), Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    unit_named(app, admin, name).await
}

fn category_names(session: &Value) -> Vec<String> {
    session["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

/// A club with an owner cookie.
async fn club(app: &TestApp, name: &str) -> (String, ClubId) {
    app.create_club_owner(&format!("Owner {name}"), name).await
}

async fn add_judge(
    app: &TestApp,
    cookie: &str,
    club_id: ClubId,
    first: &str,
    last: &str,
    trained: bool,
) -> Value {
    let response = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id, "firstname": first, "lastname": last,
                "birthdate": "1997-01-01T00:00:00.000Z", "is_trained": trained,
                "max_deployments": null, "note": null, "registrations": [],
            }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["judge_id"].clone()
}

async fn assign(
    app: &TestApp,
    admin: &str,
    judge: &Value,
    session: &Value,
    task: &str,
) -> common::TestResponse {
    app.post(
        "/api/command/assign_judge",
        json!({ "judge_id": judge, "session_id": session, "task": task }),
        Some(admin),
    )
    .await
}

async fn assign_ok(app: &TestApp, admin: &str, judge: &Value, session: &Value, task: &str) {
    let response = assign(app, admin, judge, session, task).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn plan(app: &TestApp, admin: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_plan", Some(admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["assignments"].as_array().unwrap().clone()
}

/// The id of the assignment of `judge` in `session`.
async fn assignment_id(app: &TestApp, admin: &str, judge: &Value, session: &Value) -> Value {
    plan(app, admin)
        .await
        .into_iter()
        .find(|a| &a["judge_id"] == judge && &a["session_id"] == session)
        .unwrap_or_else(|| panic!("no assignment"))["id"]
        .clone()
}

async fn split(
    app: &TestApp,
    admin: &str,
    unit: &Value,
    sessions: Value,
    keep: Option<Value>,
) -> common::TestResponse {
    let mut body = json!({ "kind": unit["kind"], "id": unit["id"], "sessions": sessions });
    if let Some(keep) = keep {
        body["keep_assignment_ids"] = keep;
    }
    app.post("/api/command/set_jury_sessions", body, Some(admin))
        .await
}

fn group(id: Option<&Value>, categories: &[CategoryId]) -> Value {
    json!({ "id": id, "category_ids": categories })
}

async fn state(app: &TestApp) -> Vec<String> {
    let mut all = vec![];
    for sql in [
        "SELECT hex(id) || hex(session_id) FROM jury_assignments ORDER BY 1",
        "SELECT hex(id) || COALESCE(needed_technical, '-') FROM jury_sessions ORDER BY 1",
        "SELECT hex(session_id) || hex(category_id) FROM jury_session_categories ORDER BY 1",
    ] {
        all.extend(texts(&app.db, sql).await);
    }
    all
}

/// A heat "Neu" with EMU17 and EMU23 split into two sessions: (unit, first
/// session, second session, admin, club cookie and club).
struct Split {
    app: TestApp,
    admin: String,
    unit: Value,
    first: Value,
    second: Value,
    emu17: CategoryId,
    emu23: CategoryId,
    club: (String, ClubId),
}

async fn split_heat() -> Split {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let club = club(&app, "RSV").await;
    let unit = new_heat(&app, &admin, "Neu", &["EMU17", "EMU23"]).await;
    let emu17 = app.category_id("EMU17").await;
    let emu23 = app.category_id("EMU23").await;
    let first = unit["sessions"][0]["id"].clone();
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&first), &[emu17]), group(None, &[emu23])]),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let unit = unit_named(&app, &admin, "Neu").await;
    let second = unit["sessions"][1]["id"].clone();
    assert_eq!(unit["sessions"][0]["id"], first);
    Split {
        app,
        admin,
        unit,
        first,
        second,
        emu17,
        emu23,
        club,
    }
}

#[tokio::test]
async fn list_jury_units_carries_one_session_per_unit() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let all = units(&app, &admin).await;
    assert!(!all.is_empty());
    for unit in &all {
        let sessions = unit["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "{unit}");
        let session = &sessions[0];
        assert_eq!(session["number"], 1);
        for task in ["technical", "performance", "dismounts"] {
            assert_eq!(
                session[format!("needed_{task}")],
                unit[format!("needed_{task}")]
            );
            assert_eq!(session[format!("override_{task}")], Value::Null);
        }
    }
    let heat = all.iter().find(|u| u["name"] == "JE EW").unwrap();
    let mut names = category_names(&heat["sessions"][0]);
    names.sort();
    assert_eq!(names, ["EWU11", "EWU13", "EWU15"]);
    // A final's session holds the categories of the heats feeding it.
    let final_unit = all.iter().find(|u| u["name"] == "Finale EW").unwrap();
    let mut names = category_names(&final_unit["sessions"][0]);
    names.sort();
    assert_eq!(names, ["EWU11", "EWU13", "EWU15", "EWU17"]);
}

#[tokio::test]
async fn splitting_a_heat_gives_two_ordered_sessions() {
    let s = split_heat().await;
    let sessions = s.unit["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["number"], 1);
    assert_eq!(sessions[1]["number"], 2);
    assert_eq!(category_names(&sessions[0]), ["EMU17"]);
    assert_eq!(category_names(&sessions[1]), ["EMU23"]);
    // The new one inherits the unit's counts.
    assert_eq!(sessions[1]["needed_technical"], s.unit["needed_technical"]);
    assert_eq!(sessions[1]["override_technical"], Value::Null);

    // The kept session keeps its assignment.
    let judge = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    assign_ok(&s.app, &s.admin, &judge, &s.first, "technical").await;
    let again = split(
        &s.app,
        &s.admin,
        &s.unit,
        json!([
            group(Some(&s.first), &[s.emu17]),
            group(Some(&s.second), &[s.emu23])
        ]),
        None,
    )
    .await;
    assert_eq!(again.status, StatusCode::OK, "{:?}", again.body);
    assert_eq!(again.body, json!({ "moved": 0, "deleted": 0 }));
    assert_eq!(plan(&s.app, &s.admin).await[0]["session_id"], s.first);

    // The timeplan decides the order: EMU23 first makes its session number 1.
    for _ in 0..3 {
        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM timeplan WHERE category_id = (SELECT id FROM categories WHERE name = 'EMU23')",
        )
        .fetch_one(&s.app.db)
        .await
        .unwrap();
        let moved = s
            .app
            .post(
                "/api/command/move_timeplan_up",
                json!({ "id": id }),
                Some(&s.admin),
            )
            .await;
        assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);
    }
    let unit = unit_named(&s.app, &s.admin, "Neu").await;
    assert_eq!(unit["sessions"][0]["id"], s.second);
    assert_eq!(unit["sessions"][0]["number"], 1);
    assert_eq!(category_names(&unit["sessions"][0]), ["EMU23"]);
    assert_eq!(unit["sessions"][1]["id"], s.first);
}

#[tokio::test]
async fn a_split_must_cover_every_category_exactly_once() {
    let s = split_heat().await;
    let foreign = s.app.category_id("EMU19").await;
    let before = state(&s.app).await;
    let cases = [
        (
            "missing category",
            json!([group(Some(&s.first), &[s.emu17])]),
            EVERY_CATEGORY_ONCE,
        ),
        (
            "twice",
            json!([
                group(Some(&s.first), &[s.emu17, s.emu23]),
                group(Some(&s.second), &[s.emu23])
            ]),
            EVERY_CATEGORY_ONCE,
        ),
        (
            "empty",
            json!([group(Some(&s.first), &[s.emu17, s.emu23]), group(None, &[])]),
            NEEDS_A_CATEGORY,
        ),
        (
            "foreign",
            json!([
                group(Some(&s.first), &[s.emu17, foreign]),
                group(Some(&s.second), &[s.emu23])
            ]),
            EVERY_CATEGORY_ONCE,
        ),
    ];
    for (label, sessions, expected) in cases {
        let response = split(&s.app, &s.admin, &s.unit, sessions, None).await;
        assert_eq!(
            response.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label}"
        );
        assert_eq!(message(&response), expected, "{label}");
        assert_eq!(state(&s.app).await, before, "{label}");
    }
}

#[tokio::test]
async fn merging_moves_only_the_kept_assignments_and_deletes_the_rest() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    let erika = add_judge(&s.app, &s.club.0, s.club.1, "Erika", "Mustermann", true).await;
    let anna = add_judge(&s.app, &s.club.0, s.club.1, "Anna", "Beispiel", true).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &erika, &s.second, "performance").await;
    assign_ok(&s.app, &s.admin, &anna, &s.second, "dismounts").await;
    let max_id = assignment_id(&s.app, &s.admin, &max, &s.first).await;
    let anna_id = assignment_id(&s.app, &s.admin, &anna, &s.second).await;

    // Both Durchgänge merge into a new one.
    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        json!([group(None, &[s.emu17, s.emu23])]),
        Some(json!([max_id, anna_id])),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({ "moved": 2, "deleted": 1 }));

    let unit = unit_named(&s.app, &s.admin, "Neu").await;
    let sessions = unit["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    let merged = &sessions[0]["id"];
    assert_ne!(merged, &s.first);
    assert_ne!(merged, &s.second);
    assert_eq!(category_names(&sessions[0]).len(), 2);
    let rows = plan(&s.app, &s.admin).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|a| &a["session_id"] == merged));
    let judges: Vec<&Value> = rows.iter().map(|a| &a["judge_id"]).collect();
    assert!(judges.contains(&&max) && judges.contains(&&anna));
    assert_eq!(
        count(&s.app.db, "SELECT COUNT(*) FROM jury_session_categories").await,
        2
    );
}

#[tokio::test]
async fn merging_into_a_surviving_session_moves_the_kept_assignments_there() {
    let s = split_heat().await;
    let erika = add_judge(&s.app, &s.club.0, s.club.1, "Erika", "Mustermann", true).await;
    assign_ok(&s.app, &s.admin, &erika, &s.second, "performance").await;
    let id = assignment_id(&s.app, &s.admin, &erika, &s.second).await;
    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        json!([group(Some(&s.first), &[s.emu17, s.emu23])]),
        Some(json!([id])),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({ "moved": 1, "deleted": 0 }));
    let rows = plan(&s.app, &s.admin).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["session_id"], s.first);
}

#[tokio::test]
async fn a_merge_that_also_splits_sends_kept_assignments_to_the_group_with_most_categories() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let club = club(&app, "RSV").await;
    let unit = new_heat(&app, &admin, "Neu", &["EMU17", "EMU19", "EMU21", "EMU23"]).await;
    let c17 = app.category_id("EMU17").await;
    let c19 = app.category_id("EMU19").await;
    let c21 = app.category_id("EMU21").await;
    let c23 = app.category_id("EMU23").await;
    let first = unit["sessions"][0]["id"].clone();
    // One Durchgang with EMU17, EMU19 and EMU21, one with EMU23.
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&first), &[c17, c19, c21]), group(None, &[c23])]),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let unit = unit_named(&app, &admin, "Neu").await;
    let second = unit["sessions"][1]["id"].clone();
    let max = add_judge(&app, &club.0, club.1, "Max", "Müller", true).await;
    assign_ok(&app, &admin, &max, &first, "technical").await;
    let id = assignment_id(&app, &admin, &max, &first).await;

    // The first Durchgang is removed: EMU17 goes to a new group, EMU19 and
    // EMU21 to the second, which receives most of its categories.
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(None, &[c17]), group(Some(&second), &[c19, c21, c23])]),
        Some(json!([id])),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({ "moved": 1, "deleted": 0 }));
    assert_eq!(plan(&app, &admin).await[0]["session_id"], second);
}

#[tokio::test]
async fn merging_refuses_a_duplicate_judge_in_the_target() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &max, &s.second, "performance").await;
    let first_id = assignment_id(&s.app, &s.admin, &max, &s.first).await;
    let second_id = assignment_id(&s.app, &s.admin, &max, &s.second).await;
    let merged = json!([group(None, &[s.emu17, s.emu23])]);
    let before = state(&s.app).await;

    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        merged.clone(),
        Some(json!([first_id, second_id])),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), DUPLICATE_JUDGE);
    assert_eq!(state(&s.app).await, before);

    // The same judge and task in both is refused the same way.
    sqlx::query("UPDATE jury_assignments SET task = 'technical'")
        .execute(&s.app.db)
        .await
        .unwrap();
    let before = state(&s.app).await;
    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        merged.clone(),
        Some(json!([first_id, second_id])),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), DUPLICATE_JUDGE);
    assert_eq!(state(&s.app).await, before);

    // With only one of them kept it succeeds.
    let response = split(&s.app, &s.admin, &s.unit, merged, Some(json!([second_id]))).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({ "moved": 1, "deleted": 1 }));
}

#[tokio::test]
async fn merging_refuses_a_duplicate_with_the_judge_already_in_the_target() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &max, &s.second, "performance").await;
    let second_id = assignment_id(&s.app, &s.admin, &max, &s.second).await;
    let before = state(&s.app).await;
    // The first survives with Max in it; the kept one would make it two.
    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        json!([group(Some(&s.first), &[s.emu17, s.emu23])]),
        Some(json!([second_id])),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), DUPLICATE_JUDGE);
    assert_eq!(state(&s.app).await, before);
}

#[tokio::test]
async fn merging_refuses_a_foreign_assignment_id() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    let erika = add_judge(&s.app, &s.club.0, s.club.1, "Erika", "Mustermann", true).await;
    let other = new_heat(&s.app, &s.admin, "Anders", &["EMU19"]).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(
        &s.app,
        &s.admin,
        &erika,
        &other["sessions"][0]["id"],
        "technical",
    )
    .await;
    let surviving = assignment_id(&s.app, &s.admin, &max, &s.first).await;
    let elsewhere = assignment_id(&s.app, &s.admin, &erika, &other["sessions"][0]["id"]).await;
    let before = state(&s.app).await;
    for (label, id) in [
        ("surviving", surviving),
        ("another unit", elsewhere),
        ("unknown", json!(Uuid::now_v7())),
    ] {
        let response = split(
            &s.app,
            &s.admin,
            &s.unit,
            json!([group(Some(&s.first), &[s.emu17, s.emu23])]),
            Some(json!([id])),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label}"
        );
        assert_eq!(message(&response), FOREIGN_ASSIGNMENT, "{label}");
        assert_eq!(state(&s.app).await, before, "{label}");
    }
}

#[tokio::test]
async fn a_judge_can_sit_in_two_sessions_of_one_unit() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &max, &s.second, "performance").await;
    let rows = plan(&s.app, &s.admin).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0]["session_id"], rows[1]["session_id"]);
    assert_eq!(rows[0]["unit_id"], rows[1]["unit_id"]);
    assert_eq!(rows[0]["unit_id"], s.unit["id"]);

    let again = assign(&s.app, &s.admin, &max, &s.first, "dismounts").await;
    assert_eq!(
        again.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        again.body
    );
    assert_eq!(message(&again), ALREADY_ASSIGNED);

    let removed = s
        .app
        .post(
            "/api/command/unassign_judge",
            json!({ "judge_id": max, "session_id": s.first }),
            Some(&s.admin),
        )
        .await;
    assert_eq!(removed.status, StatusCode::OK, "{:?}", removed.body);
    let rows = plan(&s.app, &s.admin).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["session_id"], s.second);
}

#[tokio::test]
async fn overriding_needed_counts() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let unit = unit_named(&app, &admin, "JE EW").await;
    let session = unit["sessions"][0]["id"].clone();
    let edit = |body: Value| {
        let app = &app;
        let admin = &admin;
        async move {
            app.post("/api/command/edit_jury_session", body, Some(admin))
                .await
        }
    };

    let response = edit(json!({ "id": session, "needed_technical": 2 })).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({}));
    let unit = unit_named(&app, &admin, "JE EW").await;
    let shown = &unit["sessions"][0];
    assert_eq!(shown["needed_technical"], 2);
    assert_eq!(shown["override_technical"], 2);
    assert_eq!(shown["needed_performance"], unit["needed_performance"]);
    assert_eq!(shown["override_performance"], Value::Null);

    // The unit's counts change only what is inherited.
    let edited = app
        .post(
            "/api/command/edit_jury_unit",
            json!({
                "kind": unit["kind"], "id": unit["id"], "registrable": true,
                "deployments": 2, "needed_technical": 7, "needed_performance": 4,
                "needed_dismounts": 1,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let unit = unit_named(&app, &admin, "JE EW").await;
    let shown = &unit["sessions"][0];
    assert_eq!(shown["needed_technical"], 2);
    assert_eq!(shown["needed_performance"], 4);
    assert_eq!(shown["needed_dismounts"], 1);

    // null (or absent) clears an override.
    let response = edit(json!({ "id": session, "needed_technical": null })).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let unit = unit_named(&app, &admin, "JE EW").await;
    assert_eq!(unit["sessions"][0]["needed_technical"], 7);
    assert_eq!(unit["sessions"][0]["override_technical"], Value::Null);

    let response = edit(json!({ "id": session, "needed_dismounts": -1 })).await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(
        message(&response),
        "Einsätze und benötigte JurorInnen dürfen nicht negativ sein."
    );
}

#[tokio::test]
async fn unknown_sessions_judges_and_units_are_404_and_foreign_sessions_are_refused() {
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    let missing = json!(JurySessionId::new());
    assert_eq!(
        assign(&s.app, &s.admin, &max, &missing, "technical")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        assign(
            &s.app,
            &s.admin,
            &json!(Uuid::now_v7()),
            &s.first,
            "technical"
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let response = s
        .app
        .post(
            "/api/command/edit_jury_session",
            json!({ "id": missing }),
            Some(&s.admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    let unknown_unit = json!({ "kind": "heat", "id": Uuid::now_v7() });
    assert_eq!(
        split(&s.app, &s.admin, &unknown_unit, json!([]), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // A session of another unit in the list is not found in this unit.
    let other = new_heat(&s.app, &s.admin, "Anders", &["EMU19"]).await;
    let response = split(
        &s.app,
        &s.admin,
        &s.unit,
        json!([group(
            Some(&other["sessions"][0]["id"]),
            &[s.emu17, s.emu23]
        )]),
        None,
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "{:?}",
        response.body
    );

    // The unit of an assignment is the one of its session, whatever
    // else the body says.
    assign_ok(&s.app, &s.admin, &max, &s.second, "technical").await;
    assert_eq!(plan(&s.app, &s.admin).await[0]["unit_id"], s.unit["id"]);
}

#[tokio::test]
async fn only_the_jury_lead_manages_sessions() {
    let s = split_heat().await;
    for (command, body) in [
        (
            "set_jury_sessions",
            json!({ "kind": "heat", "id": s.unit["id"], "sessions": [] }),
        ),
        ("edit_jury_session", json!({ "id": s.first })),
    ] {
        let response = s
            .app
            .post(&format!("/api/command/{command}"), body, Some(&s.club.0))
            .await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{command}");
    }
}

#[tokio::test]
async fn the_plan_carries_assignment_and_session_ids_and_a_club_sees_only_its_own_after_publishing()
{
    let s = split_heat().await;
    let (other_cookie, other_club) = club(&s.app, "RSV Andere").await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    let erika = add_judge(
        &s.app,
        &other_cookie,
        other_club,
        "Erika",
        "Mustermann",
        true,
    )
    .await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &erika, &s.second, "performance").await;

    let rows = plan(&s.app, &s.admin).await;
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(
            Uuid::parse_str(row["id"].as_str().unwrap()).is_ok(),
            "{row}"
        );
        assert!(row["session_id"] == s.first || row["session_id"] == s.second);
    }

    let own = s
        .app
        .get(
            &format!("/api/query/list_jury_plan?club_id={}", s.club.1),
            Some(&s.club.0),
        )
        .await;
    assert_eq!(own.body["assignments"], json!([]));
    s.app.close_registration().await;
    let published = s
        .app
        .post(
            "/api/command/set_jury_plan_published",
            json!({ "published": true }),
            Some(&s.admin),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{:?}", published.body);
    let own = s
        .app
        .get(
            &format!("/api/query/list_jury_plan?club_id={}", s.club.1),
            Some(&s.club.0),
        )
        .await;
    let own = own.body["assignments"].as_array().unwrap().clone();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["judge_id"], max);
    assert_eq!(own[0]["session_id"], s.first);
}

#[tokio::test]
async fn a_category_added_to_the_heat_after_the_split_shows_in_the_first_session() {
    let s = split_heat().await;
    let emu19 = s.app.category_id("EMU19").await;
    let edited = s
        .app
        .post(
            "/api/command/edit_heat",
            json!({
                "id": s.unit["id"], "new_name": "Neu", "description": null,
                "category_ids": [s.emu17, s.emu23, emu19],
            }),
            Some(&s.admin),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let unit = unit_named(&s.app, &s.admin, "Neu").await;
    let sessions = unit["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    let in_first = sessions.iter().find(|x| x["id"] == s.first).unwrap();
    let mut names = category_names(in_first);
    names.sort();
    assert_eq!(names, ["EMU17", "EMU19"]);

    // The next save has to name it.
    let response = split(
        &s.app,
        &s.admin,
        &unit,
        json!([
            group(Some(&s.first), &[s.emu17]),
            group(Some(&s.second), &[s.emu23])
        ]),
        None,
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), EVERY_CATEGORY_ONCE);
    let response = split(
        &s.app,
        &s.admin,
        &unit,
        json!([
            group(Some(&s.first), &[s.emu17]),
            group(Some(&s.second), &[s.emu23, emu19])
        ]),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

#[tokio::test]
async fn a_unit_without_categories_keeps_one_empty_session_and_cannot_be_split() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let unit = new_heat(&app, &admin, "Leer", &[]).await;
    let session = unit["sessions"][0]["id"].clone();
    assert_eq!(unit["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(category_names(&unit["sessions"][0]), Vec::<String>::new());

    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&session), &[]), group(None, &[])]),
        None,
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), NEEDS_A_CATEGORY);
    // A category is foreign.
    let emu19 = app.category_id("EMU19").await;
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&session), &[emu19])]),
        None,
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    assert_eq!(message(&response), EVERY_CATEGORY_ONCE);
    // One empty Durchgang is fine.
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&session), &[])]),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        unit_named(&app, &admin, "Leer").await["sessions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_finals_sessions_are_ordered_by_category_order_then_name() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let unit = unit_named(&app, &admin, "Finale EW").await;
    let cats: Vec<CategoryId> = {
        let mut v = vec![];
        for c in ["EWU11", "EWU13", "EWU15", "EWU17"] {
            v.push(app.category_id(c).await);
        }
        v
    };
    let first = unit["sessions"][0]["id"].clone();
    // The later categories form the existing session, the earliest a new one.
    let response = split(
        &app,
        &admin,
        &unit,
        json!([group(Some(&first), &cats[1..]), group(None, &cats[..1])]),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let unit = unit_named(&app, &admin, "Finale EW").await;
    assert_eq!(category_names(&unit["sessions"][0]), ["EWU11"]);
    assert_eq!(unit["sessions"][1]["id"], first);
}

#[tokio::test]
async fn a_unit_with_a_judge_in_two_sessions_cannot_be_deleted() {
    // ensure_unit_unused only asks whether anything is assigned.
    let s = split_heat().await;
    let max = add_judge(&s.app, &s.club.0, s.club.1, "Max", "Müller", true).await;
    assign_ok(&s.app, &s.admin, &max, &s.first, "technical").await;
    assign_ok(&s.app, &s.admin, &max, &s.second, "performance").await;
    let refused = s
        .app
        .post(
            "/api/command/delete_heat",
            json!({ "id": s.unit["id"] }),
            Some(&s.admin),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:?}", refused.body);
}
