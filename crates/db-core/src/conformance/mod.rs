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
//! To add a test: write `pub async fn name(db: &dyn Db)` in a submodule and
//! list `name` in the macro below.

use crate::{
    Db,
    auth::NewUser,
    clubs::Actor,
    ids::{ClubId, UserId},
};

mod access;
pub use access::*;

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

/// Runs every conformance test as its own `#[tokio::test]` against a fresh
/// database from `$make`, an expression that is an async constructor
/// returning a `DbHandle`.
#[macro_export]
macro_rules! conformance_tests {
    ($make:expr) => {
        mod conformance {
            #[allow(unused_imports)]
            use super::*;
            $crate::conformance_tests!(@each $make;
                club_access_decisions,
                rider_access_decisions,
                judge_access_decisions,
                act_access_decisions,
                group_access_decisions,
            );
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
