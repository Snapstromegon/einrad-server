//! Check-in on competition day.

use time::OffsetDateTime;

use crate::{DbError, clubs::RegistrationError, ids::RiderId};

/// The check-in state of a rider.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CheckIn {
    pub rider_id: RiderId,
    /// `null` if the rider hasn't checked in yet.
    #[serde(with = "time::serde::iso8601::option")]
    pub checked_in_at: Option<OffsetDateTime>,
}

#[async_trait::async_trait]
pub trait CheckInRepo {
    /// Sets the check-in time of the riders, or checks them out with
    /// `None`. Riders that already are checked in keep their earlier time.
    /// Fails as a whole with `NotFound` if one rider doesn't exist.
    async fn set_checked_in(
        &self,
        riders: &[RiderId],
        checked_in_at: Option<OffsetDateTime>,
    ) -> Result<(), RegistrationError>;

    /// The check-in state of every rider.
    async fn list_check_ins(&self) -> Result<Vec<CheckIn>, DbError>;
}
