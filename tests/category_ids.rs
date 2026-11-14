//! `id` as the primary key of categories, with deterministic UUIDv7 IDs for
//! the existing ones, and every reference following (#356, ADR 0018).

mod common;

use sqlx::SqlitePool;
use uuid::Uuid;

const BEFORE: i64 = 20261119120000;
const MIGRATION_FILE: &str = include_str!("../migrations/20261120120000_category_ids.up.sql");

/// The fixed timestamp baked into the migration, 2026-11-14T00:00:00Z.
const BASE_MS: u64 = 1_794_614_400_000;

async fn exec(db: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn texts(db: &SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

async fn ids(db: &SqlitePool, sql: &'static str) -> Vec<Vec<u8>> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_v7(id: &[u8]) -> bool {
    id.len() == 16 && id[6] >> 4 == 7 && id[8] >> 6 == 0b10
}

/// The ID the migration documents for the category at `pos` named `name`.
fn expected_id(pos: u64, name: &str) -> String {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(16, 0);
    let ts = (BASE_MS + pos).to_be_bytes();
    let mut id = [0_u8; 16];
    id[..6].copy_from_slice(&ts[2..]);
    id[6] = 0x70 | (bytes[6] & 0x0f);
    id[7] = bytes[7];
    id[8] = 0x80 | (bytes[8] & 0x3f);
    id[9..].copy_from_slice(&bytes[9..]);
    to_hex(&id)
}

/// Categories EMU17 (order 2), KG1 (order 1), Ohne (no order, no block) and
/// "Größe" (non-ASCII, order 3); a heat; two timeplan blocks and a break slot
/// in the first; a rider and an act that fall into EMU17.
async fn before() -> (SqlitePool, sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE, &db).await.unwrap();
    for sql in [
        "INSERT INTO competition (id, name, location, start_date, end_date, determining_day, \
         registration_start, registration_end, music_upload_end, fee_per_start) VALUES \
         ('x', 'Cup', 'Halle', '2027-06-01', '2027-06-02', '2027-01-01', \
          '2027-01-01T00:00:00Z', '2027-02-01T00:00:00Z', '2027-02-01T00:00:00Z', 5)",
        "INSERT INTO heats (id, name, code) VALUES (x'aa000000000000000000000000000001', 'Einzel EM', 'E EM')",
        "INSERT INTO categories (name, act_type, is_single_male, \"order\", under_age, heat_id) VALUES \
         ('EMU17', 'single', TRUE, 2, 17, x'aa000000000000000000000000000001'), \
         ('KG1', 'group_small', FALSE, 1, NULL, NULL), \
         ('Ohne', 'pair', FALSE, NULL, NULL, NULL), \
         ('Größe', 'group_large', FALSE, 3, NULL, NULL)",
        "INSERT INTO timeplan (id, kind, category, position, show_photos) VALUES (1, 'acts', 'EMU17', 1, FALSE)",
        "INSERT INTO timeplan (id, kind, category, position) VALUES (2, 'acts', 'KG1', 2)",
        "INSERT INTO timeplan (id, kind, label, duration_seconds, position) VALUES (7, 'custom', 'Pause', 600, 3)",
        "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) VALUES (1, 1, 'Umbau', 300)",
        "INSERT INTO users (id, email, email_verified, name, password) \
         VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')",
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')",
        "INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender) \
         VALUES (x'30', x'10', 'Max', 'Müller', '2012-05-05T00:00:00Z', 'male')",
        "INSERT INTO acts (id, name, act_type) VALUES (x'40', 'Kür', 'single')",
        "INSERT INTO act_participants (act_id, rider_id) VALUES (x'40', x'30')",
    ] {
        exec(&db, sql).await;
    }
    (db, migrator)
}

async fn category_ids(db: &SqlitePool) -> Vec<Vec<u8>> {
    ids(db, "SELECT id FROM categories ORDER BY name").await
}

async fn assert_references_intact(db: &SqlitePool) {
    assert_eq!(
        texts(
            db,
            "SELECT timeplan.id || ' ' || categories.name FROM timeplan \
             JOIN categories ON categories.id = timeplan.category_id ORDER BY timeplan.id"
        )
        .await,
        ["1 EMU17", "2 KG1"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM timeplan WHERE id = 7 AND category_id IS NULL AND label = 'Pause'"
        )
        .await,
        ["1"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT CAST(show_photos AS TEXT) FROM timeplan WHERE id = 1"
        )
        .await,
        ["0"]
    );
    assert_eq!(
        texts(db, "SELECT timeplan_id || ' ' || label FROM timeplan_slots").await,
        ["1 Umbau"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT heats.name FROM categories JOIN heats ON heats.id = categories.heat_id"
        )
        .await,
        ["Einzel EM"]
    );
    // The view derives the ID, and the name with it.
    assert_eq!(
        texts(
            db,
            "SELECT view_act.category || ' ' || (view_act.category_id = categories.id) \
             FROM view_act JOIN categories ON categories.name = view_act.category"
        )
        .await,
        ["EMU17 1"]
    );
}

#[tokio::test]
async fn every_category_gets_a_valid_v7_and_every_reference_survives() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    let all = category_ids(&db).await;
    assert_eq!(all.len(), 4);
    for id in &all {
        assert!(is_v7(id), "{} is not a valid v7", to_hex(id));
        assert_eq!(Uuid::from_slice(id).unwrap().get_version_num(), 7);
    }
    assert_references_intact(&db).await;
    // The sequence of the timeplan goes on after the highest ID.
    exec(
        &db,
        "INSERT INTO timeplan (kind, label, duration_seconds) VALUES ('custom', 'x', 60)",
    )
    .await;
    assert_eq!(
        texts(&db, "SELECT CAST(MAX(id) AS TEXT) FROM timeplan").await,
        ["8"]
    );
}

#[tokio::test]
async fn ids_are_the_fixed_timestamp_plus_position_with_the_name_bits() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    // By order (empty first), then name: Ohne 0, KG1 1, EMU17 2, Größe 3.
    for (pos, name) in [(0, "Ohne"), (1, "KG1"), (2, "EMU17"), (3, "Größe")] {
        let got: String =
            sqlx::query_scalar("SELECT lower(hex(id)) FROM categories WHERE name = $1")
                .bind(name)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(got, expected_id(pos, name), "{name}");
    }
}

#[tokio::test]
async fn two_copies_of_one_database_get_identical_ids() {
    let (a, migrator_a) = before().await;
    let (b, migrator_b) = before().await;
    migrator_a.run(&a).await.unwrap();
    migrator_b.run(&b).await.unwrap();

    assert_eq!(category_ids(&a).await, category_ids(&b).await);
}

/// The re-mint section of the migration, run again on the migrated table.
fn remint_sql() -> &'static str {
    let start = MIGRATION_FILE.find("-- BEGIN remint").unwrap();
    let end = MIGRATION_FILE.find("-- END remint").unwrap();
    &MIGRATION_FILE[start..end]
}

#[tokio::test]
async fn running_the_remint_again_changes_nothing() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let before_ids = category_ids(&db).await;

    // A temp table lives on one connection: run everything on the same.
    let mut conn = db.acquire().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(remint_sql().to_string()))
        .execute(&mut *conn)
        .await
        .unwrap();
    let changed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM remint r JOIN categories c ON c.name = r.name WHERE c.id != r.new_id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM remint")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!((changed, total), (0, 4));
    drop(conn);
    assert_eq!(category_ids(&db).await, before_ids);
}

#[tokio::test]
async fn categories_are_keyed_by_id_and_a_missing_id_is_an_error() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('categories') WHERE pk > 0"
        )
        .await,
        ["id"]
    );
    let err = sqlx::query("INSERT INTO categories (name, act_type) VALUES ('Neu', 'pair')")
        .execute(&db)
        .await;
    assert!(err.is_err(), "no default ID any more");
    let err = sqlx::query(
        "INSERT INTO categories (id, name, act_type) VALUES (randomblob(16), 'EMU17', 'pair')",
    )
    .execute(&db)
    .await;
    assert!(err.is_err(), "name stays unique");
    // A timeplan block must point at a category that exists.
    let err =
        sqlx::query("INSERT INTO timeplan (kind, category_id) VALUES ('acts', randomblob(16))")
            .execute(&db)
            .await;
    assert!(err.is_err(), "category_id is a foreign key");
}

#[tokio::test]
async fn a_rename_is_a_plain_update_and_keeps_the_references() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let before_ids = category_ids(&db).await;

    exec(
        &db,
        "UPDATE categories SET name = 'EM U17' WHERE name = 'EMU17'",
    )
    .await;
    assert_eq!(category_ids(&db).await.len(), before_ids.len());
    assert_eq!(
        texts(
            &db,
            "SELECT categories.name FROM timeplan JOIN categories ON categories.id = timeplan.category_id WHERE timeplan.id = 1"
        )
        .await,
        ["EM U17"]
    );
    assert_eq!(
        texts(&db, "SELECT category FROM view_act").await,
        ["EM U17"]
    );
}

#[tokio::test]
async fn deleting_a_category_with_a_block_is_refused_and_a_heat_goes_to_null() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert!(
        sqlx::query("DELETE FROM categories WHERE name = 'EMU17'")
            .execute(&db)
            .await
            .is_err()
    );
    exec(&db, "DELETE FROM heats").await;
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM categories WHERE heat_id IS NOT NULL"
        )
        .await,
        ["0"]
    );
}

#[tokio::test]
async fn the_down_migration_round_trips() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let migrated = category_ids(&db).await;

    migrator.undo(&db, BEFORE).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('categories') WHERE pk > 0"
        )
        .await,
        ["name"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT category FROM timeplan WHERE category IS NOT NULL ORDER BY id"
        )
        .await,
        ["EMU17", "KG1"]
    );
    assert_eq!(texts(&db, "SELECT category FROM view_act").await, ["EMU17"]);
    assert_eq!(
        texts(
            &db,
            "SELECT timeplan_id || ' ' || label FROM timeplan_slots"
        )
        .await,
        ["1 Umbau"]
    );

    // Up again: the same IDs, the same references.
    migrator.run(&db).await.unwrap();
    assert_eq!(category_ids(&db).await, migrated);
    assert_references_intact(&db).await;
}
