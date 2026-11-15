//! Competition commands: the competition, its settings and import.

/// The write side of the competition area of the API; one method per endpoint.
pub trait CompetitionCommands: Send + Sync {}
