//! Fee waivers of clubs, riders and acts (#253, see the glossary's Fee
//! waiver): always in full and with a reason, stored as the reason in a
//! `fee_waiver` column. Granting and revoking a waiver writes a payment log
//! entry in the same transaction (see [`crate::bookings::BookingsRepo`]).

use uuid::Uuid;

use crate::ids::{ActId, ClubId, RiderId};

/// What a waiver applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WaiverClub {
    pub id: ClubId,
    pub name: String,
}

/// A waiver as the payment log records it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// The reason of a waiver as stored: trimmed. `None` revokes the waiver; a
/// reason that is blank is refused, since a waiver is always in full and
/// with a reason.
pub fn check_waiver_reason(
    reason: Option<&str>,
) -> Result<Option<&str>, crate::error::CommandError> {
    match reason.map(str::trim) {
        Some("") => Err(crate::error::CommandError::Rejected(
            "Bitte einen Grund angeben, warum die Gebühr erlassen wird.".to_string(),
        )),
        reason => Ok(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CommandError;

    #[test]
    fn a_waiver_reason_is_trimmed_and_never_blank() {
        assert!(matches!(check_waiver_reason(None), Ok(None)));
        assert!(matches!(
            check_waiver_reason(Some("  Härtefall \n")),
            Ok(Some("Härtefall"))
        ));
        for blank in ["", "   ", "\t\n"] {
            assert!(matches!(
                check_waiver_reason(Some(blank)),
                Err(CommandError::Rejected(m)) if m.contains("Grund")
            ));
        }
    }
}
