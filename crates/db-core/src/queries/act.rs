//! Act queries: acts, songs and the startlist.

/// The read side of the act area of the API; one method per endpoint.
pub trait ActQueries: Send + Sync {}
