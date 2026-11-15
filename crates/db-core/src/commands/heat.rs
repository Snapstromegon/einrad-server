//! Heat commands: heats.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them. Names, codes and descriptions are already normalized by the handler
//! (`heats::normalize_*`).

use crate::{
    actor::Actor,
    error::CommandError,
    ids::{CategoryId, HeatId},
};

/// Adds a heat and assigns its categories (HEAT-1). A heat of groups starts
/// with its Vorlauf skipped.
#[derive(Debug, Clone)]
pub struct AddHeat {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub category_ids: Vec<CategoryId>,
    /// Without: the default for its categories, see
    /// [`default_threshold`](crate::heats::default_threshold).
    pub threshold: Option<i64>,
}

/// Edits a heat, its categories included.
#[derive(Debug, Clone)]
pub struct EditHeat {
    pub heat_id: HeatId,
    pub new_name: String,
    /// The new normalized code; without, it stays.
    pub new_code: Option<String>,
    pub description: Option<String>,
    /// The categories of the heat; the ones left out leave it.
    pub category_ids: Vec<CategoryId>,
    /// Without, it stays.
    pub threshold: Option<i64>,
}

/// Deletes a heat; its categories leave it.
#[derive(Debug, Clone)]
pub struct DeleteHeat {
    pub heat_id: HeatId,
}

/// Skips the Vorlauf of a heat or runs it again.
#[derive(Debug, Clone)]
pub struct SetHeatSkip {
    pub heat_id: HeatId,
    pub skip: bool,
}

/// The write side of the heat area of the API; one method per endpoint.
///
/// A refusal rolls everything back: `Rejected` (400), `Conflict` (409),
/// `NotFound` and `NotFoundMessage` (404) carry German messages where there
/// are any.
#[async_trait::async_trait]
pub trait HeatCommands: Send + Sync {
    async fn add_heat(&self, actor: &Actor, cmd: AddHeat) -> Result<(), CommandError>;

    async fn edit_heat(&self, actor: &Actor, cmd: EditHeat) -> Result<(), CommandError>;

    /// Refused while a final's slots or a jury unit use it.
    async fn delete_heat(&self, actor: &Actor, cmd: DeleteHeat) -> Result<(), CommandError>;

    /// `false` if it already was so. Refused once an act of the heat or its
    /// final has started.
    async fn set_heat_skip(&self, actor: &Actor, cmd: SetHeatSkip) -> Result<bool, CommandError>;
}
