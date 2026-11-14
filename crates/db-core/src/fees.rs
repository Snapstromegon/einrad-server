//! What clubs owe: start fees and the fee for missing jury deployments.
//!
//! The one place the fee rules live in the backend; the club queries, the
//! payment QR code (through them) and `/metrics` all use it.

use std::collections::HashMap;

use crate::{DbError, ids::ClubId, jury::JuryObligation};

/// What a club owes, split into its parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct AmountDue {
    /// `fee_per_start` per start, at most `max_fee_per_rider` per rider.
    pub start_fees: f64,
    /// For missing jury deployments; 0 without the jury registration.
    pub jury_fee: f64,
    pub total: f64,
    /// What fee waivers drop: the start fees of waived clubs, riders and
    /// acts, and the jury fee of a waived club.
    pub waived: f64,
}

/// What a club owes and, with the jury registration, its jury obligation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClubFees {
    pub amount_due: AmountDue,
    pub jury: Option<JuryObligation>,
}

#[async_trait::async_trait]
pub trait FeesRepo {
    /// The fees of every club under the current competition's fee rules.
    /// Clubs without riders and jury obligation owe nothing and may be
    /// missing from the map. The jury fee only applies while
    /// `jury_registration` is on and the competition has jury rules.
    async fn fees_by_club(
        &self,
        jury_registration: bool,
    ) -> Result<HashMap<ClubId, ClubFees>, DbError>;

    /// The fees of one club.
    async fn fees_of_club(
        &self,
        jury_registration: bool,
        club: ClubId,
    ) -> Result<ClubFees, DbError>;
}
