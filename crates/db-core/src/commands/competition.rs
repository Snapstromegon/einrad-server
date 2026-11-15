//! Competition commands: the competition and its settings.
//!
//! The request types are DB-facing; the handler converts its API body into
//! them after trimming and validating. Creating and importing a competition
//! belong to the seed/import area (S11).

use crate::{actor::Actor, competition::Competition, error::CommandError};

/// Changes the settings of the current competition and both colors (`None` is
/// the default), also while the registration is open.
#[derive(Debug, Clone)]
pub struct EditCompetition {
    /// The new settings, trimmed and validated by the handler. The id is
    /// ignored: it never changes, the current competition is edited.
    pub competition: Competition,
}

/// The write side of the competition area of the API; one method per endpoint.
///
/// `NotFound` if there is no current competition.
#[async_trait::async_trait]
pub trait CompetitionCommands: Send + Sync {
    async fn edit_competition(
        &self,
        actor: &Actor,
        cmd: EditCompetition,
    ) -> Result<(), CommandError>;
}
