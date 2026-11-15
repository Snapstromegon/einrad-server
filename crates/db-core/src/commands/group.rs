//! Group commands: groups (acts of several riders).

/// The write side of the group area of the API; one method per endpoint.
pub trait GroupCommands: Send + Sync {}
