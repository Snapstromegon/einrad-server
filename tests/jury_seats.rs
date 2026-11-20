//! Jury seats (#387, ADR 0020): every assignment occupies one numbered seat
//! (T1.., P1.., A1..) of its Durchgang.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::ids::{CategoryId, ClubId};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use time::{
    OffsetDateTime,
    format_description::well_known::{Iso8601, Rfc3339},
};

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
        Self::with_categories(&["EMU17", "EMU23"]).await
    }

    async fn with_categories(categories: &[&str]) -> Self {
        let app = TestApp::new(common::in_memory_migrated().await).await;
        let admin = app.create_admin().await;
        let club = app.create_club_owner("Owner RSV", "RSV").await;
        let unit = new_heat(&app, &admin, "Neu", categories).await;
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

// ---------- seat_judge ----------

impl Fixture {
    /// A judge registered for these tasks of the unit.
    async fn registered(&self, first: &str, last: &str, tasks: &[&str]) -> Value {
        let registrations: Vec<Value> = tasks
            .iter()
            .map(|task| {
                json!({ "unit_kind": self.unit["kind"], "unit_id": self.unit["id"], "task": task })
            })
            .collect();
        add_judge(
            &self.app,
            &self.club.0,
            self.club.1,
            first,
            last,
            true,
            json!(registrations),
        )
        .await
    }

    /// Splits the heat into EMU17 (the first Durchgang) and EMU23; returns the
    /// second Durchgang's id.
    async fn split_in_two(&mut self) -> Value {
        let emu17 = self.app.category_id("EMU17").await;
        let emu23 = self.app.category_id("EMU23").await;
        let response = self
            .app
            .post(
                "/api/command/set_jury_sessions",
                json!({
                    "kind": self.unit["kind"], "id": self.unit["id"],
                    "sessions": [
                        { "id": self.session, "category_ids": [emu17] },
                        { "id": null, "category_ids": [emu23] },
                    ],
                }),
                Some(&self.admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        self.reload_unit().await;
        self.unit["sessions"][1]["id"].clone()
    }

    async fn seat_judge(
        &self,
        judge: &Value,
        session: &Value,
        task: &str,
        seat: i64,
    ) -> common::TestResponse {
        self.app
            .post(
                "/api/command/seat_judge",
                json!({ "judge_id": judge, "session_id": session, "task": task, "seat": seat }),
                Some(&self.admin),
            )
            .await
    }

    /// Seats the judge in the first Durchgang and returns the outcome.
    async fn seat_ok(&self, judge: &Value, task: &str, seat: i64) -> String {
        let response = self.seat_judge(judge, &self.session, task, seat).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        response.body["outcome"].as_str().unwrap().to_string()
    }

    /// The assignment id of `judge` in `session`.
    async fn assignment_id(&self, judge: &Value, session: &Value) -> Option<Value> {
        plan(&self.app, &self.admin)
            .await
            .into_iter()
            .find(|a| &a["judge_id"] == judge && &a["session_id"] == session)
            .map(|a| a["id"].clone())
    }

    /// Registers the judge (any training) for the task of the unit, in SQL.
    async fn register_raw(&self, judge: &Value, task: &str) {
        let judge = uuid::Uuid::parse_str(judge.as_str().unwrap()).unwrap();
        let heat = uuid::Uuid::parse_str(self.unit["id"].as_str().unwrap()).unwrap();
        sqlx::query("INSERT INTO judge_registration (judge_id, heat_id, task) VALUES (?, ?, ?)")
            .bind(judge)
            .bind(heat)
            .bind(task)
            .execute(&self.app.db)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn seat_judge_assigns_to_a_free_seat() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    assert_eq!(f.seat_ok(&max, "technical", 2).await, "assigned");
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T2"));
}

#[tokio::test]
async fn a_judge_on_another_seat_moves_when_the_seat_is_empty() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    let id = f.assignment_id(&max, &f.session).await;
    assert_eq!(f.seat_ok(&max, "performance", 2).await, "moved");
    assert_eq!(plan(&f.app, &f.admin).await.len(), 1);
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("P2"));
    assert_eq!(f.assignment_id(&max, &f.session).await, id);
}

#[tokio::test]
async fn a_judge_on_another_seat_swaps_with_the_occupant_when_it_may_follow() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.registered("Erika", "Mustermann", &["technical"]).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    f.assign_ok(&erika, "performance", Some(2)).await;
    let max_id = f.assignment_id(&max, &f.session).await;
    let erika_id = f.assignment_id(&erika, &f.session).await;
    assert_eq!(f.seat_ok(&max, "performance", 2).await, "swapped");
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("P2"));
    assert_eq!(f.seat_of(&erika, &f.session).await.as_deref(), Some("T1"));
    assert_eq!(f.assignment_id(&max, &f.session).await, max_id);
    assert_eq!(f.assignment_id(&erika, &f.session).await, erika_id);
    assert_eq!(plan(&f.app, &f.admin).await.len(), 2);

    // The same task needs no registration.
    let anna = f.judge("Anna", "Beispiel", true).await;
    let jonas = f.judge("Jonas", "Becker", true).await;
    f.assign_ok(&anna, "technical", Some(2)).await;
    f.assign_ok(&jonas, "technical", Some(3)).await;
    assert_eq!(f.seat_ok(&anna, "technical", 3).await, "swapped");
    assert_eq!(f.seat_of(&anna, &f.session).await.as_deref(), Some("T3"));
    assert_eq!(f.seat_of(&jonas, &f.session).await.as_deref(), Some("T2"));
}

#[tokio::test]
async fn the_occupant_is_removed_when_it_is_not_registered_for_the_judges_task() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    f.assign_ok(&erika, "performance", Some(2)).await;
    let response = f.assign(&erika, &second, "technical", Some(1)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let max_id = f.assignment_id(&max, &f.session).await;

    assert_eq!(f.seat_ok(&max, "performance", 2).await, "moved_replaced");
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("P2"));
    assert_eq!(f.assignment_id(&max, &f.session).await, max_id);
    assert_eq!(f.seat_of(&erika, &f.session).await, None);
    let rows = plan(&f.app, &f.admin).await;
    assert!(
        !rows
            .iter()
            .any(|a| a["seat_label"] == "T1" && a["session_id"] == f.session)
    );
    // Erika's seat in the other Durchgang is untouched.
    assert_eq!(f.seat_of(&erika, &second).await.as_deref(), Some("T1"));

    // Registered for the unit's P, but not for T: the same.
    let anna = f.judge("Anna", "Beispiel", true).await;
    let jonas = f.registered("Jonas", "Becker", &["performance"]).await;
    f.assign_ok(&anna, "technical", Some(2)).await;
    f.assign_ok(&jonas, "performance", Some(1)).await;
    assert_eq!(f.seat_ok(&anna, "performance", 1).await, "moved_replaced");
    assert_eq!(f.seat_of(&jonas, &f.session).await, None);
}

#[tokio::test]
async fn an_untrained_occupant_that_cannot_follow_is_removed() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let anna = f.judge("Anna", "Beispiel", false).await;
    // Registered for technical although untrained (an old registration).
    f.register_raw(&anna, "technical").await;
    f.assign_ok(&anna, "dismounts", Some(1)).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    assert_eq!(f.seat_ok(&max, "dismounts", 1).await, "moved_replaced");
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("A1"));
    assert_eq!(f.seat_of(&anna, &f.session).await, None);
}

#[tokio::test]
async fn picking_an_unseated_judge_on_an_occupied_seat_replaces_the_occupant() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign_ok(&erika, "performance", Some(1)).await;
    assert_eq!(
        f.assign(&erika, &second, "performance", Some(1))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(f.seat_ok(&anna, "performance", 1).await, "replaced");
    assert_eq!(f.seat_of(&anna, &f.session).await.as_deref(), Some("P1"));
    assert_eq!(f.seat_of(&erika, &f.session).await, None);
    assert_eq!(f.seat_of(&erika, &second).await.as_deref(), Some("P1"));
}

#[tokio::test]
async fn seat_judge_on_the_judges_own_seat_changes_nothing() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(2)).await;
    let before = assignments_state(&f.app).await;
    assert_eq!(f.seat_ok(&max, "technical", 2).await, "unchanged");
    assert_eq!(assignments_state(&f.app).await, before);
}

#[tokio::test]
async fn seats_beyond_the_count_cannot_be_targeted() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    for (seat, label) in [(4, "T4"), (0, "T0")] {
        let response = f.seat_judge(&max, &f.session, "technical", seat).await;
        assert_eq!(response.status, REFUSED, "{:?}", response.body);
        assert_eq!(
            message(&response),
            format!("Platz {label} gibt es in diesem Durchgang nicht.")
        );
    }
    assert!(assignments_state(&f.app).await.is_empty());

    // A migrated extra seat can't be chosen, but a swap may put the occupant
    // of a real seat there: Max sits on T7 (count 3), Erika on T1.
    let erika = f.judge("Erika", "Mustermann", true).await;
    let max_uuid = uuid::Uuid::parse_str(max.as_str().unwrap()).unwrap();
    let heat = uuid::Uuid::parse_str(f.unit["id"].as_str().unwrap()).unwrap();
    let session = uuid::Uuid::parse_str(f.session.as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task, seat) \
         VALUES (?, ?, ?, 'technical', 7)",
    )
    .bind(max_uuid)
    .bind(heat)
    .bind(session)
    .execute(&f.app.db)
    .await
    .unwrap();
    f.assign_ok(&erika, "technical", Some(1)).await;
    let response = f.seat_judge(&erika, &f.session, "technical", 7).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(f.seat_ok(&max, "technical", 1).await, "swapped");
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T1"));
    assert_eq!(f.seat_of(&erika, &f.session).await.as_deref(), Some("T7"));
}

#[tokio::test]
async fn an_untrained_judge_cannot_land_on_a_technical_seat() {
    let f = Fixture::new().await;
    let anna = f.judge("Anna", "Beispiel", false).await;
    f.assign_ok(&anna, "dismounts", Some(1)).await;
    let before = assignments_state(&f.app).await;
    let response = f.seat_judge(&anna, &f.session, "technical", 2).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(
        message(&response),
        "Nicht geschulte Personen können nur Abstiege zählen."
    );
    assert_eq!(assignments_state(&f.app).await, before);
    assert_eq!(f.seat_ok(&anna, "dismounts", 2).await, "moved");
}

#[tokio::test]
async fn a_seat_in_another_durchgang_is_not_touched() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    f.assign_ok(&erika, "technical", Some(2)).await;
    assert_eq!(
        f.assign(&max, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    assert_eq!(f.seat_ok(&max, "technical", 2).await, "swapped");
    assert_eq!(f.seat_of(&max, &second).await.as_deref(), Some("T1"));
}

#[tokio::test]
async fn seat_judge_answers_404_for_unknowns_and_403_for_a_club() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    let ghost = json!("00000000-0000-7000-8000-000000000000");
    assert_eq!(
        f.seat_judge(&ghost, &f.session, "technical", 1)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.seat_judge(&max, &ghost, "technical", 1).await.status,
        StatusCode::NOT_FOUND
    );
    let response = f
        .app
        .post(
            "/api/command/seat_judge",
            json!({ "judge_id": max, "session_id": f.session, "task": "technical", "seat": 1 }),
            Some(&f.club.0),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{:?}",
        response.body
    );
    assert!(assignments_state(&f.app).await.is_empty());
}

// ---------- Never lower a needed count below an occupied seat ----------

impl Fixture {
    async fn edit_session(&self, session: &Value, technical: Option<i64>) -> common::TestResponse {
        self.app
            .post(
                "/api/command/edit_jury_session",
                json!({ "id": session, "needed_technical": technical }),
                Some(&self.admin),
            )
            .await
    }

    async fn unit_technical(&self) -> i64 {
        unit_named(&self.app, &self.admin, "Neu").await["needed_technical"]
            .as_i64()
            .unwrap()
    }
}

#[tokio::test]
async fn lowering_the_unit_count_below_an_occupied_seat_is_refused() {
    let f = Fixture::new().await;
    f.set_unit_needed(5, 2, 2, true).await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(4)).await;

    let response = f.edit_unit(3, 2, 2, 2).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert_eq!(
        message(&response),
        "Platz T4 ist noch besetzt (Max Müller). Erst den Platz freigeben, dann den Bedarf auf 3 senken."
    );
    assert_eq!(f.unit_technical().await, 5);

    assert_eq!(f.edit_unit(4, 2, 2, 2).await.status, StatusCode::OK);
    f.unassign(&max, &f.session).await;
    assert_eq!(f.edit_unit(3, 2, 2, 2).await.status, StatusCode::OK);
    assert_eq!(f.unit_technical().await, 3);
}

#[tokio::test]
async fn a_durchgang_without_override_follows_the_unit_count() {
    let mut f = Fixture::new().await;
    f.set_unit_needed(5, 2, 2, true).await;
    let second = f.split_in_two().await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign(&anna, &second, "technical", Some(5)).await;

    // Durchgang 2 inherits the unit's count.
    let response = f.edit_unit(4, 2, 2, 2).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert_eq!(
        message(&response),
        "EMU23: Platz T5 ist noch besetzt (Anna Beispiel). Erst den Platz freigeben, dann den Bedarf auf 4 senken."
    );
    assert_eq!(f.unit_technical().await, 5);

    // With an override of 5 its effective count stays 5, Durchgang 1 follows.
    assert_eq!(
        f.edit_session(&second, Some(5)).await.status,
        StatusCode::OK
    );
    assert_eq!(f.edit_unit(4, 2, 2, 2).await.status, StatusCode::OK);
    assert_eq!(f.unit_technical().await, 4);
}

#[tokio::test]
async fn lowering_or_clearing_a_durchgang_override_is_checked() {
    let mut f = Fixture::new().await;
    assert_eq!(
        f.edit_session(&f.session, Some(5)).await.status,
        StatusCode::OK
    );
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign_ok(&anna, "technical", Some(5)).await;

    for lowered in [Some(4), None] {
        let response = f.edit_session(&f.session, lowered).await;
        assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
        assert!(message(&response).starts_with("Platz T5 ist noch besetzt (Anna Beispiel)."));
    }
    // The refusals rolled back: the override is still 5.
    f.reload_unit().await;
    assert_eq!(f.unit["sessions"][0]["override_technical"], 5);
    assert_eq!(
        f.edit_session(&f.session, Some(5)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.edit_session(&f.session, Some(6)).await.status,
        StatusCode::OK
    );
    // Raising the unit's count never refuses.
    assert_eq!(f.edit_unit(9, 2, 2, 2).await.status, StatusCode::OK);
}

#[tokio::test]
async fn unrelated_edits_pass_when_migrated_seats_exceed_the_count() {
    let f = Fixture::new().await;
    f.set_unit_needed(5, 2, 2, true).await;
    let heat = uuid::Uuid::parse_str(f.unit["id"].as_str().unwrap()).unwrap();
    let session = uuid::Uuid::parse_str(f.session.as_str().unwrap()).unwrap();
    for seat in 1..=7 {
        let judge = f.judge("Gast", &format!("Nummer{seat}"), true).await;
        sqlx::query(
            "INSERT INTO jury_assignments (judge_id, heat_id, session_id, task, seat) \
             VALUES (?, ?, ?, 'technical', ?)",
        )
        .bind(uuid::Uuid::parse_str(judge.as_str().unwrap()).unwrap())
        .bind(heat)
        .bind(session)
        .bind(seat)
        .execute(&f.app.db)
        .await
        .unwrap();
    }
    // Only the deployments change; the count stays 5.
    assert_eq!(f.edit_unit(5, 2, 2, 3).await.status, StatusCode::OK);
    assert_eq!(f.edit_unit(5, 2, 2, 3).await.status, StatusCode::OK);
    let response = f.edit_unit(4, 2, 2, 3).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert!(message(&response).starts_with("Platz T7 ist noch besetzt"));
}

#[tokio::test]
async fn other_tasks_are_not_affected() {
    let f = Fixture::new().await;
    f.set_unit_needed(5, 2, 2, true).await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(4)).await;
    assert_eq!(f.edit_unit(5, 0, 2, 2).await.status, StatusCode::OK);
}

// ---------- Re-merge gives moved assignments free seats ----------

const MERGE_NO_SEATS: &str = "Im Durchgang Neu gibt es nicht genug freie Plätze für Technik: Anna Beispiel bekommt keinen Platz.";

impl Fixture {
    /// Saves the split of the unit: groups of (existing Durchgang or new,
    /// categories), keeping these assignments.
    async fn save_split(&self, groups: Value, keep: Vec<Value>) -> common::TestResponse {
        self.app
            .post(
                "/api/command/set_jury_sessions",
                json!({
                    "kind": self.unit["kind"], "id": self.unit["id"],
                    "sessions": groups, "keep_assignment_ids": keep,
                }),
                Some(&self.admin),
            )
            .await
    }

    async fn id_of(&self, judge: &Value, session: &Value) -> Value {
        self.assignment_id(judge, session).await.unwrap()
    }
}

/// No (session, task, seat) twice.
async fn no_duplicate_seats(app: &TestApp) {
    let duplicates: Vec<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jury_assignments GROUP BY session_id, task, seat HAVING COUNT(*) > 1",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(duplicates.is_empty());
}

#[tokio::test]
async fn a_moved_assignment_keeps_its_seat_when_it_is_free() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let max = f.judge("Max", "Müller", true).await;
    assert_eq!(
        f.assign(&max, &second, "technical", Some(2)).await.status,
        StatusCode::OK
    );
    let id = f.id_of(&max, &second).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;

    let response = f
        .save_split(
            json!([{ "id": f.session, "category_ids": [emu17, emu23] }]),
            vec![id],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T2"));
    no_duplicate_seats(&f.app).await;
}

#[tokio::test]
async fn a_moved_assignment_gets_the_lowest_free_seat_when_its_seat_is_taken() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&erika, "technical", Some(1)).await;
    assert_eq!(
        f.assign(&max, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    let id = f.id_of(&max, &second).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;

    let response = f
        .save_split(
            json!([{ "id": f.session, "category_ids": [emu17, emu23] }]),
            vec![id],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(f.seat_of(&erika, &f.session).await.as_deref(), Some("T1"));
    assert_eq!(f.seat_of(&max, &f.session).await.as_deref(), Some("T2"));
    no_duplicate_seats(&f.app).await;
}

#[tokio::test]
async fn two_moved_assignments_do_not_collide() {
    let mut f = Fixture::with_categories(&["EMU17", "EMU19", "EMU23"]).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu19 = f.app.category_id("EMU19").await;
    let emu23 = f.app.category_id("EMU23").await;
    let d1 = f.session.clone();
    let response = f
        .save_split(
            json!([
                { "id": d1, "category_ids": [emu17] },
                { "id": null, "category_ids": [emu19] },
                { "id": null, "category_ids": [emu23] },
            ]),
            vec![],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    f.reload_unit().await;
    let sessions: Vec<Value> = f.unit["sessions"].as_array().unwrap().clone();
    assert_eq!(sessions.len(), 3);
    let (d2, d3) = (sessions[1]["id"].clone(), sessions[2]["id"].clone());
    let erika = f.judge("Erika", "Mustermann", true).await;
    let max = f.judge("Max", "Müller", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    for (judge, session) in [(&erika, &d1), (&max, &d2), (&anna, &d3)] {
        let response = f.assign(judge, session, "technical", Some(1)).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
    let keep = vec![f.id_of(&max, &d2).await, f.id_of(&anna, &d3).await];

    let response = f
        .save_split(
            json!([{ "id": d1, "category_ids": [emu17, emu19, emu23] }]),
            keep,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // By old seat, then assignment id: Max before Anna.
    assert_eq!(f.seat_of(&erika, &d1).await.as_deref(), Some("T1"));
    assert_eq!(f.seat_of(&max, &d1).await.as_deref(), Some("T2"));
    assert_eq!(f.seat_of(&anna, &d1).await.as_deref(), Some("T3"));
    no_duplicate_seats(&f.app).await;
}

#[tokio::test]
async fn a_merge_that_leaves_no_free_seat_is_refused() {
    let mut f = Fixture::new().await;
    f.set_unit_needed(2, 2, 2, true).await;
    let second = f.split_in_two().await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    let max = f.judge("Max", "Müller", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign_ok(&erika, "technical", Some(1)).await;
    f.assign_ok(&max, "technical", Some(2)).await;
    assert_eq!(
        f.assign(&anna, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    let id = f.id_of(&anna, &second).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;
    let merged = json!([{ "id": f.session, "category_ids": [emu17, emu23] }]);
    let before = assignments_state(&f.app).await;
    let sessions_before = texts(&f.app.db, "SELECT hex(id) FROM jury_sessions ORDER BY 1").await;

    let response = f.save_split(merged.clone(), vec![id]).await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    assert_eq!(message(&response), MERGE_NO_SEATS);
    assert_eq!(assignments_state(&f.app).await, before);
    assert_eq!(
        texts(&f.app.db, "SELECT hex(id) FROM jury_sessions ORDER BY 1").await,
        sessions_before
    );

    // Not kept, it merges and Anna's assignment is deleted.
    let response = f.save_split(merged, vec![]).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(f.seat_of(&anna, &f.session).await, None);
    assert_eq!(plan(&f.app, &f.admin).await.len(), 2);
}

#[tokio::test]
async fn a_refused_merge_names_the_categories_of_a_split_target() {
    let mut f = Fixture::with_categories(&["EMU17", "EMU19", "EMU23"]).await;
    f.set_unit_needed(2, 2, 2, true).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu19 = f.app.category_id("EMU19").await;
    let emu23 = f.app.category_id("EMU23").await;
    let d1 = f.session.clone();
    let response = f
        .save_split(
            json!([
                { "id": d1, "category_ids": [emu17] },
                { "id": null, "category_ids": [emu19] },
                { "id": null, "category_ids": [emu23] },
            ]),
            vec![],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    f.reload_unit().await;
    let d2 = f.unit["sessions"][1]["id"].clone();
    let erika = f.judge("Erika", "Mustermann", true).await;
    let max = f.judge("Max", "Müller", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    f.assign_ok(&erika, "technical", Some(1)).await;
    f.assign_ok(&max, "technical", Some(2)).await;
    assert_eq!(
        f.assign(&anna, &d2, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    let id = f.id_of(&anna, &d2).await;
    let before = assignments_state(&f.app).await;

    // EMU19 joins EMU17, which has no free technical seat; EMU23 stays apart.
    let response = f
        .save_split(
            json!([
                { "id": d1, "category_ids": [emu17, emu19] },
                { "id": null, "category_ids": [emu23] },
            ]),
            vec![id],
        )
        .await;
    assert_eq!(response.status, REFUSED, "{:?}", response.body);
    let text = message(&response);
    assert!(text.contains("Im Durchgang EMU17, EMU19 "), "{text}");
    assert!(text.contains("Anna Beispiel bekommt keinen Platz"), "{text}");
    assert_eq!(assignments_state(&f.app).await, before);
}

#[tokio::test]
async fn moved_seats_follow_the_target_of_each_assignment() {
    let mut f = Fixture::with_categories(&["EMU17", "EMU19", "EMU21", "EMU23"]).await;
    let c17 = f.app.category_id("EMU17").await;
    let c19 = f.app.category_id("EMU19").await;
    let c21 = f.app.category_id("EMU21").await;
    let c23 = f.app.category_id("EMU23").await;
    let first = f.session.clone();
    let response = f
        .save_split(
            json!([
                { "id": first, "category_ids": [c17, c19, c21] },
                { "id": null, "category_ids": [c23] },
            ]),
            vec![],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    f.reload_unit().await;
    let second = f.unit["sessions"][1]["id"].clone();
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    assert_eq!(
        f.assign(&max, &first, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.assign(&erika, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    let id = f.id_of(&max, &first).await;

    // The first Durchgang goes: EMU17 to a new group, EMU19 and EMU21 to the
    // second, which receives most of its categories; Max's seat 1 is taken
    // there by Erika.
    let response = f
        .save_split(
            json!([
                { "id": null, "category_ids": [c17] },
                { "id": second, "category_ids": [c19, c21, c23] },
            ]),
            vec![id],
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(f.seat_of(&max, &second).await.as_deref(), Some("T2"));
    assert_eq!(f.seat_of(&erika, &second).await.as_deref(), Some("T1"));
    no_duplicate_seats(&f.app).await;
}

// ---------- Published plan data: seat, day, block and time ----------

fn parse_iso(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Iso8601::DEFAULT).unwrap()
}

fn parse_rfc3339(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Rfc3339).unwrap()
}

/// The predicted blocks that run one of these categories (a heat) or the
/// final, in plan order.
async fn blocks_of(
    app: &TestApp,
    categories: &[CategoryId],
    final_id: Option<&Value>,
) -> Vec<Value> {
    let response = app.get("/api/query/predict_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            let acts = &item["timeplan_entry"]["Acts"];
            match final_id {
                Some(id) => &acts["final_id"] == id,
                None => categories
                    .iter()
                    .any(|c| acts["home_category_id"] == json!(c)),
            }
        })
        .cloned()
        .collect()
}

impl Fixture {
    /// Replaces the seeded timeplan with a greeting, then these entries.
    async fn set_timeplan(&self, entries: Vec<Value>) {
        let existing = self.app.get("/api/query/list_timeplan", None).await;
        for entry in existing.body.as_array().unwrap() {
            let response = self
                .app
                .post(
                    "/api/command/delete_timeplan_entry",
                    json!({ "id": entry["id"] }),
                    Some(&self.admin),
                )
                .await;
            assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        }
        let greeting = json!({
            "earliest_start_time": "2027-04-01T09:30:00Z", "duration_seconds": 600,
            "label": "Begrüßung",
        });
        for entry in std::iter::once(greeting).chain(entries) {
            let response = self
                .app
                .post("/api/command/add_timeplan_entry", entry, Some(&self.admin))
                .await;
            assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        }
    }
}

fn category_block(id: CategoryId) -> Value {
    json!({ "earliest_start_time": null, "duration_seconds": null, "label": null, "category_id": id })
}

fn final_block(id: &Value) -> Value {
    json!({ "earliest_start_time": null, "duration_seconds": null, "label": null, "final_id": id })
}

fn pause() -> Value {
    json!({ "earliest_start_time": null, "duration_seconds": 1800, "label": "Pause" })
}

/// What the plan must say for an assignment run in this predicted block.
fn assert_block(assignment: &Value, block: &Value) {
    let start = parse_iso(&block["planned_start"]);
    assert_eq!(
        parse_rfc3339(&assignment["starts_at"]),
        start,
        "{assignment}"
    );
    assert_eq!(
        assignment["day"],
        json!(format!(
            "{:04}-{:02}-{:02}",
            start.year(),
            u8::from(start.month()),
            start.day()
        )),
        "{assignment}"
    );
    assert_eq!(
        assignment["block_title"], block["timeplan_entry"]["Acts"]["description"],
        "{assignment}"
    );
}

#[tokio::test]
async fn the_plan_carries_seat_day_block_and_time() {
    let f = Fixture::new().await;
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", Some(2)).await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;
    let blocks = blocks_of(&f.app, &[emu17, emu23], None).await;
    let first = blocks.first().expect("the heat is in the timeplan");

    let rows = plan(&f.app, &f.admin).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["seat"], 2);
    assert_eq!(row["seat_label"], "T2");
    assert_eq!(row["unit_name"], "Neu");
    assert_eq!(row["session_label"], "");
    assert_block(row, first);
}

#[tokio::test]
async fn a_split_units_durchgang_uses_its_own_blocks() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;
    f.set_timeplan(vec![category_block(emu17), pause(), category_block(emu23)])
        .await;
    let max = f.judge("Max", "Müller", true).await;
    let erika = f.judge("Erika", "Mustermann", true).await;
    f.assign_ok(&max, "technical", Some(1)).await;
    assert_eq!(
        f.assign(&erika, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );

    let block_17 = blocks_of(&f.app, &[emu17], None).await.remove(0);
    let block_23 = blocks_of(&f.app, &[emu23], None).await.remove(0);
    assert_ne!(block_17["planned_start"], block_23["planned_start"]);
    let rows = plan(&f.app, &f.admin).await;
    let of = |judge: &Value| {
        rows.iter()
            .find(|a| &a["judge_id"] == judge)
            .unwrap()
            .clone()
    };
    assert_block(&of(&max), &block_17);
    assert_eq!(of(&max)["session_label"], "EMU17");
    assert_block(&of(&erika), &block_23);
    assert_eq!(of(&erika)["session_label"], "EMU23");
}

#[tokio::test]
async fn a_final_uses_its_own_block() {
    let f = Fixture::new().await;
    let unit = unit_named(&f.app, &f.admin, "Finale EW").await;
    let session = unit["sessions"][0]["id"].clone();
    f.set_timeplan(vec![pause(), final_block(&unit["id"])])
        .await;
    let max = f.judge("Max", "Müller", true).await;
    assert_eq!(
        f.assign(&max, &session, "performance", None).await.status,
        StatusCode::OK
    );
    let blocks = blocks_of(&f.app, &[], Some(&unit["id"])).await;
    let block = blocks.first().expect("the final is in the timeplan");

    let rows = plan(&f.app, &f.admin).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["unit_name"], "Finale EW");
    assert_eq!(rows[0]["seat_label"], "P1");
    assert_block(&rows[0], block);
}

#[tokio::test]
async fn a_unit_outside_the_timeplan_has_no_day_block_or_time() {
    let f = Fixture::new().await;
    let entries = f.app.get("/api/query/list_timeplan", None).await;
    for entry in entries.body.as_array().unwrap() {
        let response = f
            .app
            .post(
                "/api/command/delete_timeplan_entry",
                json!({ "id": entry["id"] }),
                Some(&f.admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
    let max = f.judge("Max", "Müller", true).await;
    f.assign_ok(&max, "technical", None).await;
    let rows = plan(&f.app, &f.admin).await;
    assert_eq!(rows.len(), 1);
    for field in ["day", "block_title", "starts_at"] {
        assert_eq!(rows[0][field], Value::Null, "{field}");
    }
    assert_eq!(rows[0]["unit_name"], "Neu");
}

#[tokio::test]
async fn entries_are_sorted_by_time_and_a_club_sees_only_its_own_once_published() {
    let mut f = Fixture::new().await;
    let second = f.split_in_two().await;
    let emu17 = f.app.category_id("EMU17").await;
    let emu23 = f.app.category_id("EMU23").await;
    let finale = unit_named(&f.app, &f.admin, "Finale EW").await;
    let finale_session = finale["sessions"][0]["id"].clone();
    // The final runs last, although "Finale EW" sorts before "Neu".
    f.set_timeplan(vec![
        category_block(emu17),
        pause(),
        category_block(emu23),
        pause(),
        final_block(&finale["id"]),
    ])
    .await;
    let (guest_cookie, guest_club) = f.app.create_club_owner("Owner Gast", "RSV Gast").await;
    let max = f.judge("Max", "Müller", true).await;
    let anna = f.judge("Anna", "Beispiel", true).await;
    let lena = f.judge("Lena", "Kurz", true).await;
    let jonas = add_judge(
        &f.app,
        &guest_cookie,
        guest_club,
        "Jonas",
        "Becker",
        true,
        json!([]),
    )
    .await;
    // Entered in an order that is neither the time nor the seat order.
    assert_eq!(
        f.assign(&max, &second, "technical", Some(1)).await.status,
        StatusCode::OK
    );
    f.assign_ok(&jonas, "technical", Some(2)).await;
    f.assign_ok(&anna, "technical", Some(1)).await;
    assert_eq!(
        f.assign(&lena, &finale_session, "performance", None)
            .await
            .status,
        StatusCode::OK
    );

    let times = |rows: &[Value]| -> Vec<(OffsetDateTime, i64)> {
        rows.iter()
            .map(|a| (parse_rfc3339(&a["starts_at"]), a["seat"].as_i64().unwrap()))
            .collect()
    };
    let all = plan(&f.app, &f.admin).await;
    assert_eq!(all.len(), 4);
    let mut sorted = times(&all);
    sorted.sort();
    assert_eq!(times(&all), sorted);
    // Time beats the unit name: the final comes last.
    assert_eq!(all[3]["unit_name"], "Finale EW");
    assert_eq!(all[0]["unit_name"], "Neu");

    // Before publishing a club sees nothing.
    let club_plan = |cookie: &str, club: ClubId| {
        let app = &f.app;
        let uri = format!("/api/query/list_jury_plan?club_id={club}");
        let cookie = cookie.to_string();
        async move {
            let response = app.get(&uri, Some(&cookie)).await;
            assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
            response.body["assignments"].as_array().unwrap().clone()
        }
    };
    assert!(club_plan(&f.club.0, f.club.1).await.is_empty());

    f.app.close_registration().await;
    let published = f
        .app
        .post(
            "/api/command/set_jury_plan_published",
            json!({ "published": true }),
            Some(&f.admin),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{:?}", published.body);
    let own = club_plan(&f.club.0, f.club.1).await;
    assert_eq!(own.len(), 3);
    let own_judges: Vec<&Value> = own.iter().map(|a| &a["judge_id"]).collect();
    assert!(own_judges.contains(&&max) && own_judges.contains(&&anna));
    assert!(own_judges.contains(&&lena));
    let mut sorted = times(&own);
    sorted.sort();
    assert_eq!(times(&own), sorted);
    let guest = club_plan(&guest_cookie, guest_club).await;
    assert_eq!(guest.len(), 1);
    assert_eq!(guest[0]["judge_id"], jonas);
}
