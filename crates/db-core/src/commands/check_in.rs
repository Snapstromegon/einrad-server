//! CheckIn commands: the check-in desk.

/// The write side of the checkin area of the API; one method per endpoint.
pub trait CheckInCommands: Send + Sync {}
