//! Check-in commands: the check-in desk.

use time::OffsetDateTime;

use crate::{actor::Actor, error::CommandError, ids::RiderId};

/// Sets the check-in time of the riders, or checks them out with `None`.
/// Riders that already are checked in keep their earlier time.
#[derive(Debug, Clone)]
pub struct SetCheckedIn {
    pub rider_ids: Vec<RiderId>,
    pub checked_in_at: Option<OffsetDateTime>,
}

/// The write side of the check-in area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait CheckInCommands: Send + Sync {
    /// Fails as a whole with `NotFound` if one rider doesn't exist.
    async fn set_checked_in(&self, actor: &Actor, cmd: SetCheckedIn) -> Result<(), CommandError>;
}
