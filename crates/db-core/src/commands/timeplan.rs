//! Timeplan commands: the timeplan and the live schedule.

/// The write side of the timeplan area of the API; one method per endpoint.
pub trait TimeplanCommands: Send + Sync {}
