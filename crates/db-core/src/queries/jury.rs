//! Jury queries: jury units, judges and the jury plan.

/// The read side of the jury area of the API; one method per endpoint.
pub trait JuryQueries: Send + Sync {}
