//! Heats and finals have a readable name and a short code (#347, ADR 0017):
//! the migration from the single name, and what the code is used for.

mod common;

const BEFORE_READABLE_NAMES: i64 = 20261117120000;

async fn exec(db: &sqlx::SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn pairs(db: &sqlx::SqlitePool, sql: &'static str) -> Vec<(String, Option<String>)> {
    sqlx::query_as(sql).fetch_all(db).await.unwrap()
}

/// A database before the migration. Heats: "E EM 1" with a description,
/// "E EM 2" without, "KG1" and "KG2" with the same description. Finals:
/// "E EM" with a description and "E EW" without.
async fn before() -> (sqlx::SqlitePool, &'static sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = &db_sqlite::MIGRATOR;
    migrator.run_to(BEFORE_READABLE_NAMES, &db).await.unwrap();
    exec(
        &db,
        "INSERT INTO heats (name, description) VALUES \
         ('E EM 1', 'Einzel männlich – Expert 1'), ('E EM 2', NULL), \
         ('KG1', 'Kleingruppe'), ('KG2', 'Kleingruppe')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO finals (name, description, act_type, \"order\") VALUES \
         ('E EM', 'Finale Einzel männlich Expert', 'single', 1), ('E EW', '  ', 'single', 2)",
    )
    .await;
    (db, migrator)
}

#[tokio::test]
async fn the_migration_takes_the_description_as_name_and_the_old_name_as_code() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        pairs(&db, "SELECT code, name FROM heats ORDER BY code").await,
        [
            (
                "E EM 1".to_string(),
                Some("Einzel männlich – Expert 1".to_string())
            ),
            ("E EM 2".to_string(), Some("E EM 2".to_string())),
            // Two heats with the same description can't both take it.
            ("KG1".to_string(), Some("KG1".to_string())),
            ("KG2".to_string(), Some("KG2".to_string())),
        ]
    );
    assert_eq!(
        pairs(&db, "SELECT code, name FROM finals ORDER BY code").await,
        [
            (
                "E EM".to_string(),
                Some("Finale Einzel männlich Expert".to_string())
            ),
            ("E EW".to_string(), Some("E EW".to_string())),
        ]
    );
    // The description moved into the name; it stays free for an extra.
    assert_eq!(
        pairs(
            &db,
            "SELECT code, description FROM heats WHERE description IS NOT NULL ORDER BY code"
        )
        .await,
        [
            ("KG1".to_string(), Some("Kleingruppe".to_string())),
            ("KG2".to_string(), Some("Kleingruppe".to_string())),
        ]
    );
}

#[tokio::test]
async fn a_row_inserted_without_a_code_takes_its_name_as_code() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    exec(
        &db,
        "INSERT INTO heats (id, name) VALUES (x'0190f2d4b1c07abc8def0123456789a1', 'Neu')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO finals (id, name, act_type) VALUES (x'0190f2d4b1c07abc8def0123456789a2', 'Neu', 'pair')",
    )
    .await;

    assert_eq!(
        pairs(&db, "SELECT code, name FROM heats WHERE name = 'Neu'").await,
        [("Neu".to_string(), Some("Neu".to_string()))]
    );
    assert_eq!(
        pairs(&db, "SELECT code, name FROM finals WHERE name = 'Neu'").await,
        [("Neu".to_string(), Some("Neu".to_string()))]
    );
}

#[tokio::test]
async fn codes_are_unique_per_table() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    let duplicate = sqlx::query(
        "INSERT INTO heats (id, name, code) VALUES (x'0190f2d4b1c07abc8def0123456789a3', 'Anders', 'E EM 1')",
    )
        .execute(&db)
        .await;
    assert!(duplicate.is_err());
    // The same code in the other table is fine.
    exec(
        &db,
        "INSERT INTO finals (id, name, code, act_type) VALUES (x'0190f2d4b1c07abc8def0123456789a4', 'Anders', 'E EM 1', 'pair')",
    )
    .await;
}

#[tokio::test]
async fn jury_units_have_a_readable_name_and_a_code() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        pairs(
            &db,
            "SELECT code, name FROM view_jury_unit WHERE kind = 'final' ORDER BY code"
        )
        .await,
        [
            (
                "E EM".to_string(),
                Some("Finale Einzel männlich Expert".to_string())
            ),
            ("E EW".to_string(), Some("E EW".to_string())),
        ]
    );
}

#[tokio::test]
async fn reverting_the_migration_writes_the_code_back_as_name() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    // A rename after the migration (swapping names with another heat's code
    // must not trip over the unique name while reverting).
    exec(&db, "UPDATE heats SET name = 'tmp' WHERE code = 'E EM 1'").await;
    exec(
        &db,
        "UPDATE heats SET name = 'E EM 1' WHERE code = 'E EM 2'",
    )
    .await;
    exec(
        &db,
        "UPDATE heats SET name = 'E EM 2' WHERE code = 'E EM 1'",
    )
    .await;

    migrator.undo(&db, BEFORE_READABLE_NAMES).await.unwrap();

    assert_eq!(
        pairs(&db, "SELECT name, description FROM heats ORDER BY name").await,
        [
            ("E EM 1".to_string(), Some("E EM 2".to_string())),
            ("E EM 2".to_string(), Some("E EM 1".to_string())),
            ("KG1".to_string(), Some("Kleingruppe".to_string())),
            ("KG2".to_string(), Some("Kleingruppe".to_string())),
        ]
    );
    assert_eq!(
        pairs(&db, "SELECT name, description FROM finals ORDER BY name").await,
        [
            (
                "E EM".to_string(),
                Some("Finale Einzel männlich Expert".to_string())
            ),
            ("E EW".to_string(), Some("  ".to_string())),
        ]
    );
}
