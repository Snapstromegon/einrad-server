//! Jury seats (#387, ADR 0020): every assignment occupies one numbered seat
//! (T1.., P1.., A1..) of its Durchgang.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::ids::{CategoryId, ClubId};
use serde_json::{Value, json};
use sqlx::SqlitePool;

const BEFORE: i64 = 20261122120000;

const HEAT: &str = "x'aa000000000000000000000000000001'";
const FINAL: &str = "x'cc000000000000000000000000000003'";
const SESSION_OLD: &str = "x'0190f2d4b1c07abc8def0123456789a1'";
const SESSION_YOUNG: &str = "x'0190f2d4b1c07abc8def0123456789a2'";
const SESSION_FINAL: &str = "x'0190f2d4b1c07abc8def0123456789a3'";

async fn exec(db: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn try_exec(db: &SqlitePool, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .map(|_| ())
}

async fn count(db: &SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.unwrap()
}

/// A database just before the seat migration: a heat with two sessions, a
/// final with one, four judges (Max Müller, Erika Mustermann, Anna Beispiel,
/// Jonas Becker).
async fn before() -> (SqlitePool, sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE, &db).await.unwrap();
    for sql in [
        format!("INSERT INTO heats (id, name, code) VALUES ({HEAT}, 'Einzel EM', 'E EM')"),
        format!("INSERT INTO finals (id, name, code, act_type, \"order\") VALUES ({FINAL}, 'Finale EM', 'F EM', 'single', 1)"),
        "INSERT INTO users (id, email, email_verified, name, password) VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')".to_string(),
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')".to_string(),
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES \
         (x'20', x'10', 'Max', 'Müller', '1990-01-01T00:00:00Z', TRUE), \
         (x'21', x'10', 'Erika', 'Mustermann', '1991-01-01T00:00:00Z', TRUE), \
         (x'22', x'10', 'Anna', 'Beispiel', '1992-01-01T00:00:00Z', TRUE), \
         (x'23', x'10', 'Jonas', 'Becker', '1993-01-01T00:00:00Z', TRUE)".to_string(),
        format!("INSERT INTO jury_sessions (id, heat_id) VALUES ({SESSION_OLD}, {HEAT}), ({SESSION_YOUNG}, {HEAT})"),
        format!("INSERT INTO jury_sessions (id, final_id) VALUES ({SESSION_FINAL}, {FINAL})"),
    ] {
        exec(&db, &sql).await;
    }
    (db, migrator)
}

/// Inserts an assignment before the migration (ids ascend with `n`).
async fn old_assignment(db: &SqlitePool, n: u8, judge: &str, session: &str, task: &str) {
    let (column, unit) = if session == SESSION_FINAL {
        ("final_id", FINAL)
    } else {
        ("heat_id", HEAT)
    };
    exec(
        db,
        &format!(
            "INSERT INTO jury_assignments (id, judge_id, {column}, session_id, task) \
             VALUES (x'0190f2d4b1c07abc8def0123456789{n:02x}', {judge}, {unit}, {session}, '{task}')"
        ),
    )
    .await;
}

async fn seats(db: &SqlitePool) -> Vec<(Vec<u8>, String, i64, i64)> {
    // (judge, task, seat, session as last byte)
    let rows: Vec<(Vec<u8>, String, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT judge_id, task, seat, session_id FROM jury_assignments \
         ORDER BY session_id, task, seat",
    )
    .fetch_all(db)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(j, t, s, sess)| (j, t, s, *sess.last().unwrap() as i64))
        .collect()
}

#[tokio::test]
async fn the_migration_numbers_existing_assignments_per_session_and_task() {
    let (db, migrator) = before().await;
    old_assignment(&db, 1, "x'20'", SESSION_OLD, "technical").await;
    old_assignment(&db, 2, "x'21'", SESSION_OLD, "technical").await;
    old_assignment(&db, 3, "x'22'", SESSION_OLD, "technical").await;
    old_assignment(&db, 4, "x'23'", SESSION_OLD, "performance").await;
    old_assignment(&db, 5, "x'20'", SESSION_YOUNG, "technical").await;
    old_assignment(&db, 6, "x'21'", SESSION_FINAL, "performance").await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        seats(&db).await,
        [
            (vec![0x23], "performance".to_string(), 1, 0xa1),
            (vec![0x20], "technical".to_string(), 1, 0xa1),
            (vec![0x21], "technical".to_string(), 2, 0xa1),
            (vec![0x22], "technical".to_string(), 3, 0xa1),
            (vec![0x20], "technical".to_string(), 1, 0xa2),
            (vec![0x21], "performance".to_string(), 1, 0xa3),
        ]
    );
}

#[tokio::test]
async fn existing_assignments_may_exceed_the_needed_count() {
    let (db, migrator) = before().await;
    exec(
        &db,
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES \
         (x'30', x'10', 'A', 'A', '1990-01-01T00:00:00Z', TRUE), (x'31', x'10', 'B', 'B', '1990-01-01T00:00:00Z', TRUE), \
         (x'32', x'10', 'C', 'C', '1990-01-01T00:00:00Z', TRUE)",
    )
    .await;
    for (n, judge) in [
        "x'20'", "x'21'", "x'22'", "x'23'", "x'30'", "x'31'", "x'32'",
    ]
    .into_iter()
    .enumerate()
    {
        old_assignment(&db, n as u8 + 1, judge, SESSION_OLD, "technical").await;
    }
    migrator.run(&db).await.unwrap();
    let numbers: Vec<i64> = sqlx::query_scalar("SELECT seat FROM jury_assignments ORDER BY seat")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6, 7]);
}

#[tokio::test]
async fn a_seat_is_unique_per_session_and_task_and_at_least_one() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let insert = |judge: &str, session: &str, task: &str, seat: &str| {
        format!(
            "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task, seat) \
             VALUES ({judge}, {HEAT}, {session}, '{task}', {seat})"
        )
    };
    exec(&db, &insert("x'20'", SESSION_OLD, "technical", "1")).await;
    assert!(
        try_exec(&db, &insert("x'21'", SESSION_OLD, "technical", "1"))
            .await
            .is_err()
    );
    exec(&db, &insert("x'21'", SESSION_YOUNG, "technical", "1")).await;
    exec(&db, &insert("x'21'", SESSION_OLD, "performance", "1")).await;
    assert!(
        try_exec(&db, &insert("x'22'", SESSION_OLD, "dismounts", "0"))
            .await
            .is_err()
    );
    assert!(
        try_exec(&db, &insert("x'22'", SESSION_OLD, "dismounts", "NULL"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_down_migration_drops_the_seat_and_keeps_the_rest() {
    let (db, migrator) = before().await;
    old_assignment(&db, 1, "x'20'", SESSION_OLD, "technical").await;
    old_assignment(&db, 2, "x'21'", SESSION_OLD, "technical").await;
    old_assignment(&db, 3, "x'22'", SESSION_YOUNG, "performance").await;
    migrator.run(&db).await.unwrap();
    // A gap in the numbering: the down migration must not mind.
    exec(
        &db,
        "UPDATE jury_assignments SET seat = 4 WHERE judge_id = x'21'",
    )
    .await;

    migrator.undo(&db, BEFORE).await.unwrap();

    assert_eq!(count(&db, "SELECT COUNT(*) FROM jury_assignments").await, 3);
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('jury_assignments')")
            .fetch_all(&db)
            .await
            .unwrap();
    assert!(!columns.contains(&"seat".to_string()), "{columns:?}");
    assert!(columns.contains(&"id".to_string()));
    assert!(columns.contains(&"session_id".to_string()));
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'jury_assignments_seat'"
        )
        .await,
        0
    );
    // One judge once per session still holds.
    assert!(
        try_exec(
            &db,
            &format!(
                "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task) \
                 VALUES (x'20', {HEAT}, {SESSION_OLD}, 'dismounts')"
            )
        )
        .await
        .is_err()
    );

    migrator.run(&db).await.unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM jury_assignments").await, 3);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM jury_assignments WHERE seat >= 1").await,
        3
    );
}

#[tokio::test]
async fn the_rebuild_keeps_the_cascade_to_judge() {
    let (db, migrator) = before().await;
    old_assignment(&db, 1, "x'20'", SESSION_OLD, "technical").await;
    migrator.run(&db).await.unwrap();
    exec(&db, "DELETE FROM judge WHERE id = x'20'").await;
    assert_eq!(count(&db, "SELECT COUNT(*) FROM jury_assignments").await, 0);
}

// ---------- Seat rules (HTTP) ----------

/// The German text of an `ErrorMessages` refusal (the API answers 500 with
/// the message Debug-quoted) or a `Conflict` (409, plain).
fn message(response: &common::TestResponse) -> String {
    response.body["message"]
        .as_str()
        .unwrap()
        .trim_matches('"')
        .to_string()
}

async fn texts(db: &SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

/// Everything about the assignments, to see that a refusal changed nothing.
async fn assignments_state(app: &TestApp) -> Vec<String> {
    texts(
        &app.db,
        "SELECT hex(id) || hex(judge_id) || hex(session_id) || task || seat FROM jury_assignments ORDER BY 1",
    )
    .await
}

async fn unit_named(app: &TestApp, cookie: &str, name: &str) -> Value {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
        .clone()
}

async fn new_heat(app: &TestApp, admin: &str, name: &str, categories: &[&str]) -> Value {
    let mut ids: Vec<CategoryId> = vec![];
    for category in categories {
        ids.push(app.category_id(category).await);
    }
    let response = app
        .post(
            "/api/command/add_heat",
            json!({ "name": name, "description": null, "category_ids": ids }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    unit_named(app, admin, name).await
}

async fn add_judge(
    app: &TestApp,
    cookie: &str,
    club_id: ClubId,
    first: &str,
    last: &str,
    trained: bool,
    registrations: Value,
) -> Value {
    let response = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id, "firstname": first, "lastname": last,
                "birthdate": "1997-01-01T00:00:00.000Z", "is_trained": trained,
                "max_deployments": null, "note": null, "registrations": registrations,
            }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["judge_id"].clone()
}

async fn plan(app: &TestApp, admin: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_plan", Some(admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["assignments"].as_array().unwrap().clone()
}

/// A heat "Neu" (EMU17, EMU23) with T3, P2, A2, an admin and a club.
struct Fixture {
    app: TestApp,
    admin: String,
    unit: Value,
    session: Value,
    club: (String, ClubId),
}

impl Fixture {
    async fn new() -> Self {
        let app = TestApp::new(common::in_memory_migrated().await).await;
        let admin = app.create_admin().await;
        let club = app.create_club_owner("Owner RSV", "RSV").await;
        let unit = new_heat(&app, &admin, "Neu", &["EMU17", "EMU23"]).await;
        let session = unit["sessions"][0]["id"].clone();
        let f = Fixture {
            app,
            admin,
            unit,
            session,
            club,
        };
        f.set_unit_needed(3, 2, 2, true).await;
        f
    }

    async fn reload_unit(&mut self) {
        self.unit = unit_named(&self.app, &self.admin, "Neu").await;
    }

    async fn edit_unit(
        &self,
        technical: i64,
        performance: i64,
        dismounts: i64,
        deployments: i64,
    ) -> common::TestResponse {
        self.app
            .post(
                "/api/command/edit_jury_unit",
                json!({
                    "kind": self.unit["kind"], "id": self.unit["id"], "registrable": true,
                    "deployments": deployments, "needed_technical": technical,
                    "needed_performance": performance, "needed_dismounts": dismounts,
                }),
                Some(&self.admin),
            )
            .await
    }

    async fn set_unit_needed(&self, technical: i64, performance: i64, dismounts: i64, ok: bool) {
        let response = self.edit_unit(technical, performance, dismounts, 2).await;
        if ok {
            assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        }
    }

    async fn judge(&self, first: &str, last: &str, trained: bool) -> Value {
        add_judge(
            &self.app,
            &self.club.0,
            self.club.1,
            first,
            last,
            trained,
            json!([]),
        )
        .await
    }

    async fn assign(
        &self,
        judge: &Value,
        session: &Value,
        task: &str,
        seat: Option<i64>,
    ) -> common::TestResponse {
        let mut body = json!({ "judge_id": judge, "session_id": session, "task": task });
        if let Some(seat) = seat {
            body["seat"] = json!(seat);
        }
        self.app
            .post("/api/command/assign_judge", body, Some(&self.admin))
            .await
    }

    async fn assign_ok(&self, judge: &Value, task: &str, seat: Option<i64>) {
        let response = self.assign(judge, &self.session, task, seat).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    async fn unassign(&self, judge: &Value, session: &Value) {
        let response = self
            .app
            .post(
                "/api/command/unassign_judge",
                json!({ "judge_id": judge, "session_id": session }),
                Some(&self.admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    /// The seat label of `judge` in `session`, if seated.
    async fn seat_of(&self, judge: &Value, session: &Value) -> Option<String> {
        plan(&self.app, &self.admin)
            .await
            .into_iter()
            .find(|a| &a["judge_id"] == judge && &a["session_id"] == session)
            .map(|a| a["seat_label"].as_str().unwrap().to_string())
    }
}

const REFUSED: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;

#[tokio::test]
async fn assigning_without_a_seat_takes_the_first_free_one() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    for judge in [&max, &erika, &anna] {
        f.assign_ok(judge, "technical", None).await;
    }
    let rows = plan(&f.app, &f.admin).await;
    let seats: Vec<(i64, &str)> = rows
        .iter()
        .map(|a| {
            (
                a["seat"].as_i64().unwrap(),
                a["seat_label"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(seats, [(1, "T1"), (2, "T2"), (3, "T3")]);
    assert_eq!(rows[0]["judge_id"], max);

    // The gap of an unassigned seat is reused.
    f.unassign(&erika, &f.session).await;
    let jonas = f.judge("Jonas", "Becker", true).await;
    f.assign_ok(&jonas, "technical", None).await;
    assert_eq!(f.seat_of(&jonas, &f.session).await.as_deref(), Some("T2"));
}

#[tokio::test]
async fn assigning_to_an_explicit_seat() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(3)).await;
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T3"));
}

#[tokio::test]
async fn a_taken_seat_is_refused() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    f.assign_ok(&max, "technical", Some(3)).await;
    let before = assignments_state(&f.app).await;
    let response = f.assign(&erika, &f.session, "technical", Some(3)).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert_eq!(message(&response), "Platz T3 ist schon besetzt.");
    assert_eq!(assignments_state(&f.app).await, before);
}

#[tokio::test]
async fn a_seat_beyond_the_count_is_refused() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    for (seat, label) in [(4, "T4"), (0, "T0")] {
        let response = f.assign(&max, &f.session, "technical", Some(seat)).await;
        assert_eq!(response.status, REFUSED, "{:?}", response.body);
        assert_eq!(
            message(&response),
            format!("Platz {label} gibt es in diesem Durchgang nicht.")
        );
    }
    assert!(assignments_state(&f.app).await.is_empty());
}

#[tokio::test]
async fn a_full_task_refuses_another_judge() {
    let f = Fixture::new().await;
    f.set_unit_needed(2, 2, 0, true).await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign_ok(&max, "technical", None).await;
    f.assign_ok(&erika, "technical", None).await;
    let before = assignments_state(&f.app).await;
    let response = f.assign(&anna, &f.session, "technical", None).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(message(&response), "Alle Plätze für Technik sind besetzt.");
    // Needed 0: the same for the task.
    let response = f.assign(&anna, &f.session, "dismounts", None).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(message(&response), "Alle Plätze für Abstiege sind besetzt.");
    assert_eq!(assignments_state(&f.app).await, before);
}

#[tokio::test]
async fn seats_are_numbered_per_task_and_per_session() {
    let mut f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    f.assign_ok(&max, "technical", None).await;
    f.assign_ok(&erika, "performance", None).await;
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T1"));
    assert_eq!(f.seat_of(&erika, &f.session).await.as_deref(), Some("P1"));

    // A second Durchgang: seat 1 is free again for Max.
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;
    let response = f
        .app
        .post(
            "/api/command/set_jury_sessions",
            json!({
                "kind": f.unit["kind"], "id": f.unit["id"],
                "sessions": [
                    { "id": f.session, "category_ids": [emu17] },
                    { "id": null, "category_ids": [emu23] },
                ],
            }),
            Some(&f.admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    f.reload_unit().await;
    let second = f.unit["sessions"][1]["id"].clone();
    let response = f.assign(&max, &second, "technical", Some(1)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(f.seat_of(&max, &second).await.as_deref(), Some("T1"));
}

#[tokio::test]
async fn an_untrained_judge_can_sit_on_a_dismount_seat() {
    let f = Fixture::new().await;
    let anna = f.judge("Anna", "Beispiel", false).await;
    f.assign_ok(&anna, "dismounts", Some(1)).await;
    assert_eq!(f.seat_of(&anna, &f.session).await.as_deref(), Some("A1"));
    let jonas = f.judge("Jonas", "Becker", false).await;
    let response = f.assign(&jonas, &f.session, "technical", Some(1)).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(
        message(&response),
        "Nicht geschulte Personen können nur Abstiege zählen."
    );
}

#[tokio::test]
async fn unknown_judge_or_session_and_duplicates_keep_their_answers() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let ghost = json!("00000000-0000-7000-8000-000000000000");
    assert_eq!(
        f.assign(&ghost, &f.session, "technical", None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.assign(&max, &ghost, "technical", None).await.status,
        StatusCode::NOT_FOUND
    );
    f.assign_ok(&max, "technical", None).await;
    let response = f.assign(&max, &f.session, "performance", None).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(message(&response), "Die JurorIn ist hier schon eingeteilt.");
}
