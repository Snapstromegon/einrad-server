//! Jury sessions (Jurydurchgänge, #386, ADR 0019): the migration gives every
//! heat and final one session, the default session of new units, and the
//! orphan rule for categories that join later.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::{
    ids::{CategoryId, JurySessionId, JuryUnitId},
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
