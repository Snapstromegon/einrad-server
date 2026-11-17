//! Fee waivers of clubs, riders and acts (#253, see the glossary's Fee
//! waiver): always in full and with a reason, stored as the reason in a
//! `fee_waiver` column. Granting and revoking a waiver writes a payment log
//! entry in the same transaction (see [`crate::bookings::BookingsRepo`]).

use uuid::Uuid;

use crate::ids::{ActId, ClubId, RiderId};

/// What a waiver applies to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WaiverLevel {
    /// All the club's fees: start fees and jury fee.
    Club,
    /// All the rider's starts.
    Rider,
    /// The act's start, for every rider.
    Act,
}

/// The target of a waiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Club(ClubId),
    Rider(RiderId),
    Act(ActId),
}

impl Target {
    #[must_use]
    pub fn level(self) -> WaiverLevel {
        match self {
            Target::Club(_) => WaiverLevel::Club,
            Target::Rider(_) => WaiverLevel::Rider,
            Target::Act(_) => WaiverLevel::Act,
        }
    }

    #[must_use]
    pub fn id(self) -> Uuid {
        match self {
            Target::Club(id) => id.0,
            Target::Rider(id) => id.0,
            Target::Act(id) => id.0,
        }
    }
}

/// A club a waiver concerns, by its name at the time.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct WaiverClub {
    pub id: ClubId,
    pub name: String,
}

/// A waiver as the payment log records it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct FeeWaiver {
    pub level: WaiverLevel,
    pub target_id: Uuid,
    /// The club's or rider's name, or the act's name (its riders' names
    /// without one), at the time.
    pub name: String,
    /// The clubs whose fees it changes: the club itself, the rider's club or
    /// the clubs of the act's riders.
    pub clubs: Vec<WaiverClub>,
    pub reason: String,
}
