//! Admin queries: administration.

/// The read side of the admin area of the API; one method per endpoint.
pub trait AdminQueries: Send + Sync {}
