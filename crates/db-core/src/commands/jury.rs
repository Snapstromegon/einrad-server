//! Jury commands: jury units, judges and the jury plan.

/// The write side of the jury area of the API; one method per endpoint.
pub trait JuryCommands: Send + Sync {}
