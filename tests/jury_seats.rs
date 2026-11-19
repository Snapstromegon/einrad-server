//! Jury seats (#387, ADR 0020): every assignment occupies one numbered seat
//! (T1.., P1.., A1..) of its Durchgang.

mod common;

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
