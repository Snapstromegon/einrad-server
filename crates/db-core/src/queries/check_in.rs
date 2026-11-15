//! CheckIn queries: the check-in desk.

/// The read side of the checkin area of the API; one method per endpoint.
pub trait CheckInQueries: Send + Sync {}
