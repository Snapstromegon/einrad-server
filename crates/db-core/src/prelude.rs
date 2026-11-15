//! One import for everything that talks to the database:
//! `use db_core::prelude::*;`.
//!
//! Rust needs a trait in scope to call its methods, even through a supertrait
//! object, so this puts every area trait in scope next to [`Db`] and
//! [`DbHandle`].

pub use crate::commands::{
    ActCommands, AdminCommands, CategoryCommands, CheckInCommands, ClubCommands, Commands,
    CompetitionCommands, FinalCommands, GroupCommands, HeatCommands, JuryCommands, MediaCommands,
    PaymentCommands, TimeplanCommands, UserCommands,
};
pub use crate::queries::{
    ActQueries, AdminQueries, CategoryQueries, CheckInQueries, ClubQueries, CompetitionQueries,
    FinalQueries, GroupQueries, HeatQueries, JuryQueries, MediaQueries, PaymentQueries, Queries,
    TimeplanQueries, UserQueries,
};
pub use crate::support::Support;
pub use crate::{Db, DbHandle};

#[cfg(test)]
mod tests {
    use super::*;

    /// A handler-like function: area methods are called through the handle
    /// with only the prelude imported (plus the request type).
    async fn needs_all_traits(db: &DbHandle, actor: &crate::actor::Actor) {
        fn commands(_: &dyn Commands) {}
        fn queries(_: &dyn Queries) {}
        fn support(_: &dyn Support) {}
        commands(db.as_ref());
        queries(db.as_ref());
        support(db.as_ref());
        // A real area method, through the handle.
        let _ = db
            .leave_pair(
                actor,
                crate::commands::club::LeavePair {
                    rider_id: crate::ids::RiderId::new(),
                },
            )
            .await;
    }

    #[test]
    fn the_prelude_is_enough_to_use_a_handle() {
        let _ = needs_all_traits;
    }
}
