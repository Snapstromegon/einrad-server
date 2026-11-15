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

    /// Every `pub async fn name(db: &dyn Db)` in this directory is a test and
    /// has to be in `conformance_list!`, else it would silently never run.
    #[test]
    fn every_conformance_test_is_registered() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/conformance");
        let mut written = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                // Only test functions take the database; `World::seed` and
                // helpers don't match.
                if let Some(rest) = line.strip_prefix("pub async fn ")
                    && let Some((name, args)) = rest.split_once('(')
                    && args.starts_with("db: &dyn Db)")
                {
                    written.push(name.to_string());
                }
            }
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
