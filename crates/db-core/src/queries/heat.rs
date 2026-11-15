//! Heat queries: heats.

/// The read side of the heat area of the API; one method per endpoint.
pub trait HeatQueries: Send + Sync {}
