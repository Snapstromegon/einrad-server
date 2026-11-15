//! Club queries: clubs, riders, pairs, acts of a club, judges, mails.

/// The read side of the club area of the API; one method per endpoint.
pub trait ClubQueries: Send + Sync {}
