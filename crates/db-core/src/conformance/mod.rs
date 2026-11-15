//! Backend conformance tests (#238): the same tests, written against
//! `&dyn Db`, run against every backend.
//!
//! A backend crate enables the `conformance` feature of `db-core` for its
//! tests and runs [`conformance_tests!`](crate::conformance_tests) with a
//! constructor of a fresh, empty, migrated database:
//!
//! ```ignore
//! db_core::conformance_tests!(|| async { SqliteRepo::in_memory().await.into_handle() });
//! ```
//!
//! Rules for tests here: they only use trait methods (no SQL, no pool), seed
//! their data through the trait, and share nothing between tests.
//!
//! To add a test: write `pub async fn name(db: &dyn Db)` in a submodule,
//! `pub use` the module and list `name` in `conformance_list!` below. That
//! list is the only registration; the `registered_tests` unit test fails when
//! a `pub async fn` of this directory is missing from it.

use crate::{
    Db,
    auth::NewUser,
    clubs::Actor,
    ids::{ClubId, UserId},
};

mod access;
mod fees;
mod registration;
mod structure;
mod timeplan;
pub use access::*;
pub use fees::*;
pub use registration::*;
pub use structure::*;
pub use timeplan::*;

/// A shared set of users, clubs and actors to seed a test from.
pub struct World {
    /// The organizer: may manage every club.
    pub organizer: Actor,
    /// Owner of club A, assigned to it.
    pub owner_a: Actor,
    pub club_a: ClubId,
    /// Owner of club B, assigned to it.
    pub owner_b: Actor,
    pub club_b: ClubId,
    /// A second user assigned to club A (not its owner).
    pub member_a: Actor,
    /// A user without a club.
    pub stranger: Actor,
}

impl World {
    /// Creates the users and the two clubs through the trait.
    pub async fn seed(db: &dyn Db) -> Self {
        let organizer = Self::user(db, "Erika Musterfrau").await;
        let owner_a_id = Self::user(db, "Max Müller").await;
        let owner_b_id = Self::user(db, "Maria Beispiel").await;
        let member_a_id = Self::user(db, "Jonas Schmidt").await;
        let stranger_id = Self::user(db, "Lea Fischer").await;
        let club_a = db.create_club(owner_a_id, "RSV Adorf").await.unwrap();
        let club_b = db.create_club(owner_b_id, "RC Beistadt").await.unwrap();
        let actor = |user_id: UserId, club_id: Option<ClubId>| Actor {
            user_id,
            club_id,
            manage_registrations: false,
        };
        Self {
            organizer: Actor {
                manage_registrations: true,
                ..actor(organizer, None)
            },
            owner_a: actor(owner_a_id, Some(club_a)),
            club_a,
            owner_b: actor(owner_b_id, Some(club_b)),
            club_b,
            member_a: actor(member_a_id, Some(club_a)),
            stranger: actor(stranger_id, None),
        }
    }

    async fn user(db: &dyn Db, name: &str) -> UserId {
        let id = UserId::new();
        let email = format!("{id}@example.org");
        db.register_user(
            NewUser {
                id,
                name,
                email: &email,
                password_hash: "not-a-real-hash",
            },
            uuid::Uuid::now_v7(),
        )
        .await
        .unwrap();
        id
    }
}

/// The one list of conformance tests. Calls `$mac!($($prefix)* names...)`
/// with every test name (as `a, b, c,`).
#[doc(hidden)]
#[macro_export]
macro_rules! conformance_list {
    ($($mac:tt)::+, $($prefix:tt)*) => {
        $($mac)::+! {$($prefix)*
            club_access_decisions,
            rider_access_decisions,
            judge_access_decisions,
            act_access_decisions,
            group_access_decisions,
            rider_names_are_unique_per_club,
            single_on_and_off,
            pair_requests_match_waiting_riders,
            link_pair_rules,
            leave_and_dissolve_pairs,
            replace_act_rider_rules,
            timeplan_forward_and_backward,
            timeplan_entries_move_and_insert,
            timeplan_started_block_keeps_its_order,
            fees_and_amount_due,
            jury_obligation_counts_acts_and_judges,
            jury_obligation_edge_cases,
            group_delete_and_member_rules,
            structure_names_and_codes_are_unique,
            heats_and_finals_share_a_discipline,
            units_with_judges_are_not_deletable,
            judge_names_are_unique_per_club,
            final_placement_skip_and_start_rules,
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! conformance_names {
    ($($name:ident,)*) => { &[$(stringify!($name)),*] };
}

/// The names of all registered tests, from [`conformance_list!`].
pub const REGISTERED: &[&str] = conformance_list!(crate::conformance_names,);

/// Runs every conformance test as its own `#[tokio::test]` against a fresh
/// database from `$make`, an expression that is an async constructor
/// returning a `DbHandle`.
#[macro_export]
macro_rules! conformance_tests {
    ($make:expr) => {
        mod conformance {
            #[allow(unused_imports)]
            use super::*;
            $crate::conformance_list!($crate::conformance_tests, @each $make;);
        }
    };
    (@each $make:expr; $($name:ident,)*) => {
        $(
            #[tokio::test]
            async fn $name() {
                let db: $crate::DbHandle = ($make)().await;
                $crate::conformance::$name(db.as_ref()).await;
            }
        )*
    };
}

#[cfg(test)]
mod registered_tests {
    use super::REGISTERED;

    /// The names of the conformance tests written in `source`: every
    /// `pub async fn` at column 0 whose only parameter is a `&dyn` reference
    /// to `Db` (whatever the parameter name, a path in front of `Db` or line
    /// breaks). A column-0 `pub` function that mentions `dyn` but is not
    /// exactly that is an error, instead of a test that silently never runs;
    /// helpers are `pub(super)` or indented.
    fn scan_tests(source: &str) -> Result<Vec<String>, String> {
        let lines: Vec<&str> = source.lines().collect();
        let mut names = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if !line.starts_with("pub ") {
                continue;
            }
            // The header runs to the body or the end of the item.
            let header: String = lines[index..]
                .iter()
                .scan(false, |done, line| {
                    if *done {
                        return None;
                    }
                    *done = line.contains('{') || line.contains(';');
                    Some(*line)
                })
                .collect::<Vec<_>>()
                .join(" ");
            let header = header.split(['{', ';']).next().unwrap_or("").to_string();
            if !header.contains("dyn") {
                continue;
            }
            let near_miss = |why: &str| {
                Err(format!(
                    "line {}: `{}` looks like a conformance test but {why}; \
                     use `pub async fn name(db: &dyn Db)` or make the helper `pub(super)`",
                    index + 1,
                    line.trim()
                ))
            };
            let compact: String = header.chars().filter(|c| !c.is_whitespace()).collect();
            let Some(rest) = compact.strip_prefix("pubasyncfn") else {
                return near_miss("is not a `pub async fn`");
            };
            let Some((name, params)) = rest.split_once('(') else {
                return near_miss("has no parameter list");
            };
            let Some((params, _)) = params.split_once(')') else {
                return near_miss("has no parameter list");
            };
            let params = params.trim_end_matches(',');
            let Some((_, ty)) = params.split_once(':') else {
                return near_miss("takes no typed parameter");
            };
            let is_db = ty
                .strip_prefix("&dyn")
                .is_some_and(|path| path == "Db" || path.ends_with("::Db"));
            if !is_db || name.is_empty() {
                return near_miss("does not take exactly `&dyn Db`");
            }
            names.push(name.to_string());
        }
        Ok(names)
    }

    #[test]
    fn the_scanner_finds_tests_and_rejects_near_misses() {
        let scan = |source| scan_tests(source);
        assert_eq!(scan("pub async fn a(db: &dyn Db) {\n}").unwrap(), ["a"]);
        assert_eq!(scan("pub async fn b(database:&dyn  Db){}").unwrap(), ["b"]);
        assert_eq!(scan("pub async fn c(db: &dyn crate::Db) {").unwrap(), ["c"]);
        assert_eq!(
            scan("pub async fn d(db: &dyn crate::Db,\n) {").unwrap(),
            ["d"]
        );
        assert_eq!(
            scan("pub async fn e(\n    db: &dyn Db,\n) {\n}").unwrap(),
            ["e"]
        );
        // Not tests: helpers, methods, other functions.
        assert!(
            scan("pub(super) async fn h(db: &dyn Db, x: u8) {}\n    pub async fn m(db: &dyn Db) {}\nasync fn p(db: &dyn Db) {}\npub fn plain(x: u8) {}")
                .unwrap()
                .is_empty()
        );
        // Near misses fail.
        for source in [
            "pub async fn f(db: &dyn Db, extra: u8) {}",
            "pub async fn g(db: &mut dyn Db) {}",
            "pub async fn h(db: Arc<dyn Db>) {}",
            "pub fn i(db: &dyn Db) {}",
            "pub async fn j(db: &dyn Database) {}",
            "pub async fn k(_: &dyn Db) -> u8 {\n}\npub async fn l(&dyn Db) {}",
        ] {
            assert!(scan(source).is_err(), "{source}");
        }
    }

    /// Every conformance test written in this directory has to be in
    /// `conformance_list!`, else it would silently never run.
    #[test]
    fn every_conformance_test_is_registered() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/conformance");
        let mut written = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            written.extend(
                scan_tests(&source).unwrap_or_else(|error| panic!("{}: {error}", path.display())),
            );
        }
        written.sort();
        let mut registered: Vec<String> = REGISTERED.iter().map(ToString::to_string).collect();
        registered.sort();
        assert_eq!(
            written, registered,
            "conformance tests written (left) vs listed in conformance_list! (right)"
        );
    }
}
