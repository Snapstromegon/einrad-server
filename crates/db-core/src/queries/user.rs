//! User queries: users, login and registration.

/// The read side of the user area of the API; one method per endpoint.
pub trait UserQueries: Send + Sync {}
