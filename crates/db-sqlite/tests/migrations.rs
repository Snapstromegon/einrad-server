//! The migrations are applied in databases that already exist, and sqlx
//! refuses to start when a checksum of an applied one changed. Moving the
//! folder (or reformatting a file) must not alter any of them.
//!
//! `migration-checksums.txt` was generated before the move into this crate:
//! `<version> <up|down> <sha384 of the file>`, which is how sqlx computes
//! `Migration::checksum`.

use db_sqlite::MIGRATOR;
use sqlx::migrate::MigrationType;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn migration_checksums_are_unchanged() {
    let expected: Vec<String> = include_str!("migration-checksums.txt")
        .lines()
        .map(str::to_owned)
        .collect();
    let mut actual: Vec<String> = MIGRATOR
        .iter()
        .map(|m| {
            let kind = match m.migration_type {
                MigrationType::ReversibleUp => "up",
                MigrationType::ReversibleDown => "down",
                MigrationType::Simple => "simple",
            };
            format!("{} {kind} {}", m.version, hex(&m.checksum))
        })
        .collect();
    actual.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(actual, expected);
}
