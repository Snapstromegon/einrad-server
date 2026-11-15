//! Final commands: finals.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them. Names and codes are already normalized, the fields
//! [`FinalFields::normalize`]d by the handler.

use crate::{
    actor::Actor,
    error::CommandError,
    finals::{FinalFields, SlotPlacement},
    ids::FinalId,
};

/// Adds a final with its slots.
#[derive(Debug, Clone)]
pub struct AddFinal {
    pub name: String,
    pub code: String,
    pub fields: FinalFields,
}

/// Edits a final; its slots are only created anew when the slot groups
/// change.
#[derive(Debug, Clone)]
pub struct EditFinal {
    pub final_id: FinalId,
    pub new_name: String,
    /// The new normalized code; without, it stays.
    pub new_code: Option<String>,
    pub fields: FinalFields,
}

/// Deletes a final with its slots and timeplan block.
#[derive(Debug, Clone)]
pub struct DeleteFinal {
    pub final_id: FinalId,
}

/// Places acts in the slots of a final (`None` empties a slot); the slots
/// not named keep theirs.
#[derive(Debug, Clone)]
pub struct SetFinalPlacement {
    pub final_id: FinalId,
    pub changes: Vec<SlotPlacement>,
}

/// Publishes the placement of a final, or withdraws it.
#[derive(Debug, Clone)]
pub struct SetFinalPublished {
    pub final_id: FinalId,
    pub published: bool,
}

/// The write side of the final area of the API; one method per endpoint.
///
/// A refusal rolls everything back, see [`HeatCommands`](super::HeatCommands).
#[async_trait::async_trait]
pub trait FinalCommands: Send + Sync {
    async fn add_final(&self, actor: &Actor, cmd: AddFinal) -> Result<(), CommandError>;

    async fn edit_final(&self, actor: &Actor, cmd: EditFinal) -> Result<(), CommandError>;

    /// Refused once it has started or while a jury unit uses it.
    async fn delete_final(&self, actor: &Actor, cmd: DeleteFinal) -> Result<(), CommandError>;

    /// The placement is checked as a whole.
    async fn set_final_placement(
        &self,
        actor: &Actor,
        cmd: SetFinalPlacement,
    ) -> Result<(), CommandError>;

    /// Not possible to withdraw once the final has started.
    async fn set_final_published(
        &self,
        actor: &Actor,
        cmd: SetFinalPublished,
    ) -> Result<(), CommandError>;
}
