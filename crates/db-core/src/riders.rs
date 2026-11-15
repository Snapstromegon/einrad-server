//! Riders, their singles and pairs: the [`RidersRepo`] trait and its types.
//!
//! A rider's starts are read from acts and act participants only; a rider
//! waiting for a pair partner who isn't linked yet has a pair request instead
//! (ADR 0004). Every change of singles, pairs and pair requests goes through
//! here, so a rider has at most one single and either one pair act or one
//! pair request. Groups are set in [`crate::groups`].

use crate::{
    DbError,
    ids::{ActId, RiderId},
};

/// A rider's single and pair starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Starts {
    /// The rider's single act.
    pub single: Option<ActId>,
    pub pair: Pair,
}

/// Where a rider stands in the pair competition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    None,
    /// Waiting for a partner (Paaranfrage): the name the club entered, if any.
    Requested {
        partner_name: Option<String>,
    },
    /// In a pair act with `partner`.
    Linked {
        act_id: ActId,
        partner: Option<RiderId>,
    },
}

/// What of the rider area is not an API endpoint (#238): the rider and pair
/// endpoints are in [`crate::commands::ClubCommands`] and
/// [`crate::queries::ClubQueries`]. Moves to `Support` with the demo seed.
#[async_trait::async_trait]
pub trait RidersRepo {
    /// The rider's single and pair starts.
    async fn rider_starts(&self, rider: RiderId) -> Result<Starts, DbError>;
}
