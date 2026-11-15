//! Competition queries: the competition and its settings.
//!
//! The read models carry no serialization; the handler maps them to its
//! response types.

use crate::{actor::Actor, competition::Competition, error::QueryError};

/// What the system status page shows from the database, read in one snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemStatus {
    /// The current competition; `None` until one is configured.
    pub competition: Option<Competition>,
    /// Whether an admin published the start list and the timeplan.
    pub startlist_published: bool,
}

/// The read side of the competition area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait CompetitionQueries: Send + Sync {
    /// The current competition with its photo settings (and password), to
    /// edit. `NotFound` without a competition.
    async fn get_competition_settings(&self, actor: &Actor) -> Result<Competition, QueryError>;

    /// Public: the current competition (if any) and whether the start list is
    /// published.
    async fn get_system_status(&self) -> Result<SystemStatus, QueryError>;
}
