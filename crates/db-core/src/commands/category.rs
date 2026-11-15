//! Category commands: categories.

/// The write side of the category area of the API; one method per endpoint.
pub trait CategoryCommands: Send + Sync {}
