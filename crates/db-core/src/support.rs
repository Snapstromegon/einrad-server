//! Everything the backend does for callers that are not API endpoints:
//! background jobs, extractors, the demo seed and the like.

/// Not an API area: the DB access of the server's own plumbing (session
/// lookup, metrics, normalization queue, seeding, ...). Filled in step by step
/// with the migration (#238).
pub trait Support: Send + Sync {}
