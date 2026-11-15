//! Act commands: acts, songs and the startlist.

/// The write side of the act area of the API; one method per endpoint.
pub trait ActCommands: Send + Sync {}
