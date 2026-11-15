//! Heat commands: heats.

/// The write side of the heat area of the API; one method per endpoint.
pub trait HeatCommands: Send + Sync {}
