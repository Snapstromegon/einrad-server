//! Admin commands: administration.

/// The write side of the admin area of the API; one method per endpoint.
pub trait AdminCommands: Send + Sync {}
