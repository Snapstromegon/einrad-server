//! Club commands: clubs, riders, pairs, acts of a club, judges, mails.

/// The write side of the club area of the API; one method per endpoint.
pub trait ClubCommands: Send + Sync {}
