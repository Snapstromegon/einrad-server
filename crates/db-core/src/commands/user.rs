//! User commands: users, login and registration.

/// The write side of the user area of the API; one method per endpoint.
pub trait UserCommands: Send + Sync {}
