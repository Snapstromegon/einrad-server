//! `id` as the primary key of heats and finals, and the deterministic re-mint
//! of the IDs of heats, finals, final slots and bookings as UUIDv7 (#355,
//! ADR 0018).

mod common;

use sqlx::SqlitePool;
use uuid::Uuid;

const BEFORE: i64 = 20261118120000;
const MIGRATION_FILE: &str = include_str!("../migrations/20261119120000_uuid_primary_keys.up.sql");

/// The fixed timestamp baked into the migration, 2026-11-14T00:00:00Z.
const BASE_MS: u64 = 1_794_614_400_000;

/// Already valid v7 IDs of the fixture; the migration must leave them alone.
const V7_HEAT: &str = "0190f2d4b1c07abc8def0123456789ab";
const V7_SLOT: &str = "0190f2d4b1c07abc9def0123456789ab";
const V7_BOOKING: &str = "0190f2d4b1c07abcbdef0123456789ab";

async fn exec(db: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn texts(db: &SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_v7(id: &[u8]) -> bool {
    id.len() == 16 && id[6] >> 4 == 7 && id[8] >> 6 == 0b10
}

async fn ids(db: &SqlitePool, sql: &'static str) -> Vec<Vec<u8>> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

/// A database before the migration. Heats E EM (random ID), KG (an ID with
/// version 4) and Jugend (already v7); finals Finale EM (random) and Finale
/// KG (a version 7 ID with the wrong variant, so not valid); slots, bookings,
/// registrations, assignments, categories and the timeplan refer to them.
async fn before() -> (SqlitePool, sqlx::migrate::Migrator) {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(BEFORE, &db).await.unwrap();
    for sql in [
        "INSERT INTO heats (id, name, code) VALUES \
         (x'aa000000000000000000000000000001', 'Einzel EM', 'E EM'), \
         (x'bb0000000000400000000000000000cc', 'Kleine Gruppen', 'KG'), \
         (x'0190f2d4b1c07abc8def0123456789ab', 'Jugend', 'JU')",
        "INSERT INTO finals (id, name, code, act_type, \"order\") VALUES \
         (x'cc000000000000000000000000000002', 'Finale EM', 'F EM', 'single', 2), \
         (x'0190f2d4b1c07abc0def0123456789ab', 'Finale KG', 'F KG', 'group_small', 1)",
        "INSERT INTO categories (name, act_type, is_single_male, heat_id) VALUES \
         ('EMU17', 'single', TRUE, x'aa000000000000000000000000000001'), \
         ('KG1', 'group_small', FALSE, x'bb0000000000400000000000000000cc'), \
         ('Ohne', 'pair', FALSE, NULL)",
        "INSERT INTO final_slots (id, final_id, position, number, heat_id) VALUES \
         (x'a1', x'cc000000000000000000000000000002', 1, 1, x'aa000000000000000000000000000001'), \
         (x'0190f2d4b1c07abc9def0123456789ab', x'cc000000000000000000000000000002', 2, 2, x'aa000000000000000000000000000001'), \
         (x'a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3', x'0190f2d4b1c07abc0def0123456789ab', 1, 1, x'bb0000000000400000000000000000cc')",
        "INSERT INTO timeplan (id, kind, final_id, position) VALUES (1, 'acts', x'cc000000000000000000000000000002', 1)",
        "INSERT INTO timeplan_slots (timeplan_id, position, heat_id, number) VALUES \
         (1, 1, x'aa000000000000000000000000000001', 2)",
        "INSERT INTO users (id, email, email_verified, name, password) \
         VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')",
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')",
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES \
         (x'20', x'10', 'Max', 'Müller', '1990-01-01T00:00:00Z', TRUE), \
         (x'21', x'10', 'Erika', 'Musterfrau', '1991-01-01T00:00:00Z', TRUE)",
        "INSERT INTO judge_registration (judge_id, heat_id, task) VALUES \
         (x'20', x'aa000000000000000000000000000001', 'technical')",
        "INSERT INTO judge_registration (judge_id, final_id, task) VALUES \
         (x'21', x'cc000000000000000000000000000002', 'performance')",
        "INSERT INTO jury_assignments (judge_id, heat_id, task) VALUES \
         (x'20', x'0190f2d4b1c07abc8def0123456789ab', 'technical')",
        "INSERT INTO jury_assignments (judge_id, final_id, task) VALUES \
         (x'21', x'cc000000000000000000000000000002', 'performance')",
        "INSERT INTO bookings (id, amount, date, club_id) VALUES \
         (x'dd000000000000000000000000000003', 15, '2027-01-01', x'10'), \
         (x'0190f2d4b1c07abcbdef0123456789ab', 20, '2027-01-02', x'10'), \
         (x'ee', -5, '2027-01-03', NULL)",
        "INSERT INTO booking_log (booking_id, at, action, after) VALUES \
         (x'dd000000000000000000000000000003', '2027-01-01T00:00:00Z', 'created', '{}'), \
         (x'ee', '2027-01-03T00:00:00Z', 'created', '{}')",
    ] {
        exec(&db, sql).await;
    }
    (db, migrator)
}

/// Every ID the migration touches, in a stable order.
async fn all_ids(db: &SqlitePool) -> Vec<Vec<Vec<u8>>> {
    vec![
        ids(db, "SELECT id FROM heats ORDER BY name").await,
        ids(db, "SELECT id FROM finals ORDER BY name").await,
        ids(
            db,
            "SELECT id FROM final_slots ORDER BY position, number, final_id",
        )
        .await,
        ids(db, "SELECT id FROM bookings ORDER BY date").await,
        ids(db, "SELECT booking_id FROM booking_log ORDER BY at").await,
    ]
}

async fn assert_references_intact(db: &SqlitePool) {
    assert_eq!(
        texts(
            db,
            "SELECT heats.name || ' ' || categories.name FROM categories \
             JOIN heats ON heats.id = categories.heat_id ORDER BY categories.name"
        )
        .await,
        ["Einzel EM EMU17", "Kleine Gruppen KG1"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM categories WHERE name = 'Ohne' AND heat_id IS NULL"
        )
        .await,
        ["1"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT finals.name || ' ' || heats.name || ' ' || final_slots.number FROM final_slots \
             JOIN finals ON finals.id = final_slots.final_id \
             JOIN heats ON heats.id = final_slots.heat_id ORDER BY finals.name, position"
        )
        .await,
        [
            "Finale EM Einzel EM 1",
            "Finale EM Einzel EM 2",
            "Finale KG Kleine Gruppen 1"
        ]
    );
    assert_eq!(
        texts(
            db,
            "SELECT finals.name FROM timeplan JOIN finals ON finals.id = timeplan.final_id"
        )
        .await,
        ["Finale EM"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT heats.name || ' ' || timeplan_slots.number FROM timeplan_slots \
             JOIN heats ON heats.id = timeplan_slots.heat_id"
        )
        .await,
        ["Einzel EM 2"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT judge.firstname || ' ' || heats.name || ' ' || task FROM judge_registration \
             JOIN judge ON judge.id = judge_id JOIN heats ON heats.id = heat_id"
        )
        .await,
        ["Max Einzel EM technical"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT judge.firstname || ' ' || finals.name FROM judge_registration \
             JOIN judge ON judge.id = judge_id JOIN finals ON finals.id = final_id"
        )
        .await,
        ["Erika Finale EM"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT judge.firstname || ' ' || heats.name FROM jury_assignments \
             JOIN judge ON judge.id = judge_id JOIN heats ON heats.id = heat_id"
        )
        .await,
        ["Max Jugend"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT judge.firstname || ' ' || finals.name FROM jury_assignments \
             JOIN judge ON judge.id = judge_id JOIN finals ON finals.id = final_id"
        )
        .await,
        ["Erika Finale EM"]
    );
    // The log still finds its bookings.
    assert_eq!(
        texts(
            db,
            "SELECT CAST(bookings.amount AS TEXT) FROM booking_log \
             JOIN bookings ON bookings.id = booking_log.booking_id ORDER BY booking_log.at"
        )
        .await,
        ["15.0", "-5.0"]
    );
    assert_eq!(
        texts(
            db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM pragma_foreign_key_check"
        )
        .await,
        ["0"]
    );
}

#[tokio::test]
async fn every_id_is_valid_v7_and_every_reference_survives() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    for group in all_ids(&db).await {
        for id in group {
            assert!(is_v7(&id), "{} is not a valid v7", to_hex(&id));
        }
    }
    assert_references_intact(&db).await;
}

#[tokio::test]
async fn ids_that_are_valid_v7_stay_and_the_others_get_the_fixed_timestamp_plus_position() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    // Already valid: untouched.
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM heats WHERE name = 'Jugend'"
        )
        .await,
        [V7_HEAT]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM final_slots WHERE position = 2"
        )
        .await,
        [V7_SLOT]
    );
    assert_eq!(
        texts(&db, "SELECT lower(hex(id)) FROM bookings WHERE amount = 20").await,
        [V7_BOOKING]
    );
    // Heats by rowid: "Einzel EM" is the first, "Kleine Gruppen" the second.
    // The old ID's random bytes are kept, with version and variant set.
    let expected = |pos: u64, old: &str| {
        let mut bytes: Vec<u8> = (0..old.len() / 2)
            .map(|i| u8::from_str_radix(&old[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        bytes.resize(16, 0);
        let ts = (BASE_MS + pos).to_be_bytes();
        let mut id = [0_u8; 16];
        id[..6].copy_from_slice(&ts[2..]);
        id[6] = 0x70 | (bytes[6] & 0x0f);
        id[7] = bytes[7];
        id[8] = 0x80 | (bytes[8] & 0x3f);
        id[9..].copy_from_slice(&bytes[9..]);
        to_hex(&id)
    };
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM heats WHERE name = 'Einzel EM'"
        )
        .await,
        [expected(0, "aa000000000000000000000000000001")]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM heats WHERE name = 'Kleine Gruppen'"
        )
        .await,
        [expected(1, "bb0000000000400000000000000000cc")]
    );
    // Finals by their order: "Finale KG" (order 1) is position 0.
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM finals WHERE name = 'Finale KG'"
        )
        .await,
        [expected(0, "0190f2d4b1c07abc0def0123456789ab")]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT lower(hex(id)) FROM finals WHERE name = 'Finale EM'"
        )
        .await,
        [expected(1, "cc000000000000000000000000000002")]
    );
    // A short ID (not valid either) is padded with zeros: rowid 1, position 0.
    assert_eq!(
        texts(&db, "SELECT lower(hex(id)) FROM final_slots WHERE number = 1 AND position = 1 AND heat_id IN (SELECT id FROM heats WHERE name = 'Einzel EM')").await,
        [expected(0, "a1")]
    );
    // Valid IDs are real UUIDs to the uuid crate, too.
    for id in ids(&db, "SELECT id FROM heats").await {
        assert_eq!(Uuid::from_slice(&id).unwrap().get_version_num(), 7);
    }
}

#[tokio::test]
async fn two_copies_of_one_database_get_identical_ids() {
    let (a, migrator_a) = before().await;
    let (b, migrator_b) = before().await;
    migrator_a.run(&a).await.unwrap();
    migrator_b.run(&b).await.unwrap();

    assert_eq!(all_ids(&a).await, all_ids(&b).await);
}

/// The re-mint section of the migration, run again on the migrated tables.
fn remint_sql() -> &'static str {
    let start = MIGRATION_FILE.find("-- BEGIN remint").unwrap();
    let end = MIGRATION_FILE.find("-- END remint").unwrap();
    &MIGRATION_FILE[start..end]
}

#[tokio::test]
async fn running_the_remint_again_changes_nothing() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let ids_before = all_ids(&db).await;

    // A temp table lives on one connection: run everything on the same.
    let mut conn = db.acquire().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(remint_sql().to_string()))
        .execute(&mut *conn)
        .await
        .unwrap();
    let changed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM remint WHERE new_id != old_id")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM remint")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!((changed, total), (0, 11));
    drop(conn);
    assert_eq!(all_ids(&db).await, ids_before);
}

#[tokio::test]
async fn heats_and_finals_are_keyed_by_id_and_a_missing_id_is_an_error() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('heats') WHERE pk > 0"
        )
        .await,
        ["id"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('finals') WHERE pk > 0"
        )
        .await,
        ["id"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE '%default_id'"
        )
        .await,
        Vec::<String>::new()
    );
    assert!(
        sqlx::query("INSERT INTO heats (name, code) VALUES ('Neu', 'N')")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("INSERT INTO finals (name, code, act_type) VALUES ('Neu', 'N', 'pair')")
            .execute(&db)
            .await
            .is_err()
    );
    // Names and codes stay unique.
    let id = Uuid::now_v7();
    assert!(
        sqlx::query("INSERT INTO heats (id, name, code) VALUES ($1, 'Einzel EM', 'X')")
            .bind(id)
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("INSERT INTO heats (id, name, code) VALUES ($1, 'Anders', 'E EM')")
            .bind(id)
            .execute(&db)
            .await
            .is_err()
    );
    // The jury view reads the rebuilt tables.
    assert_eq!(
        texts(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM view_jury_unit").await,
        ["5"]
    );
}

#[tokio::test]
async fn deleting_keeps_the_on_delete_behaviour_of_the_references() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();

    exec(&db, "DELETE FROM timeplan_slots").await;
    exec(
        &db,
        "DELETE FROM final_slots WHERE heat_id IN (SELECT id FROM heats WHERE name = 'Einzel EM')",
    )
    .await;
    exec(&db, "DELETE FROM judge_registration").await;
    exec(&db, "DELETE FROM jury_assignments").await;
    exec(&db, "DELETE FROM heats WHERE name = 'Einzel EM'").await;
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(heat_id IS NULL AS TEXT) FROM categories WHERE name = 'EMU17'"
        )
        .await,
        ["1"]
    );
    exec(&db, "DELETE FROM finals WHERE name = 'Finale KG'").await;
    assert_eq!(
        texts(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM final_slots").await,
        ["0"]
    );
    exec(&db, "DELETE FROM finals WHERE name = 'Finale EM'").await;
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM timeplan WHERE final_id IS NOT NULL"
        )
        .await,
        ["0"]
    );
}

#[tokio::test]
async fn the_down_migration_round_trips() {
    let (db, migrator) = before().await;
    migrator.run(&db).await.unwrap();
    let migrated = all_ids(&db).await;

    migrator.undo(&db, BEFORE).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('heats') WHERE pk > 0"
        )
        .await,
        ["name"]
    );
    assert_eq!(
        texts(
            &db,
            "SELECT name FROM pragma_table_info('finals') WHERE pk > 0"
        )
        .await,
        ["name"]
    );
    // The IDs stay, and the references with them.
    assert_eq!(all_ids(&db).await, migrated);
    assert_references_intact(&db).await;
    // Inserting without an ID works again, the trigger fills it in.
    exec(&db, "INSERT INTO heats (name) VALUES ('Ohne ID')").await;
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(id IS NOT NULL AS TEXT) FROM heats WHERE name = 'Ohne ID'"
        )
        .await,
        ["1"]
    );
    exec(&db, "DELETE FROM heats WHERE name = 'Ohne ID'").await;

    // Up again: nothing changes.
    migrator.run(&db).await.unwrap();
    assert_eq!(all_ids(&db).await, migrated);
    assert_references_intact(&db).await;
}
