//! Timeplan queries: the timeplan and the live schedule.

/// The read side of the timeplan area of the API; one method per endpoint.
pub trait TimeplanQueries: Send + Sync {}
