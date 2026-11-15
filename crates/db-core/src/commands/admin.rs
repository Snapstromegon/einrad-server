//! Admin commands: administration.

use crate::{actor::Actor, error::CommandError, ids::ActId};

/// Withdraws an act or takes it back.
#[derive(Debug, Clone)]
pub struct SetActWithdrawn {
    pub act_id: ActId,
    pub withdrawn: bool,
}

/// The write side of the admin area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait AdminCommands: Send + Sync {
    /// Withdraws an act after the registration deadline, or takes it back
    /// (the withdrawal date of an act that is already withdrawn stays).
    /// `NotFound` if the act doesn't exist.
    async fn set_act_withdrawn(
        &self,
        actor: &Actor,
        cmd: SetActWithdrawn,
    ) -> Result<(), CommandError>;
}
