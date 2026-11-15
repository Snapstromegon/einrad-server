//! Check-in queries: the check-in desk.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::OffsetDateTime;

use crate::{actor::Actor, error::QueryError, ids::RiderId};

/// The check-in state of a rider.
#[derive(Debug, Clone)]
pub struct CheckIn {
    pub rider_id: RiderId,
    /// `None` if the rider hasn't checked in yet.
    pub checked_in_at: Option<OffsetDateTime>,
}

/// The read side of the check-in area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait CheckInQueries: Send + Sync {
    /// The check-in state of every rider.
    async fn list_check_ins(&self, actor: &Actor) -> Result<Vec<CheckIn>, QueryError>;
}
