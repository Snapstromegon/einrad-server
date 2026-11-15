//! Category commands: categories.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them.

use crate::{act_type::ActType, actor::Actor, error::CommandError, ids::CategoryId};

/// What admins set for a category.
#[derive(Debug, Clone)]
pub struct CategoryFields {
    pub name: String,
    pub description: Option<String>,
    pub min_age: Option<f64>,
    pub under_age: Option<f64>,
    pub act_type: ActType,
    pub is_single_male: bool,
    pub einfahrzeit_seconds: i32,
    pub act_duration_seconds: i32,
    pub judge_duration_seconds: i32,
}

/// Adds a category at the end of the order.
#[derive(Debug, Clone)]
pub struct AddCategory {
    pub fields: CategoryFields,
}

/// Overwrites a category (a rename included).
#[derive(Debug, Clone)]
pub struct EditCategory {
    pub category_id: CategoryId,
    pub fields: CategoryFields,
}

/// Deletes a category with the timeplan entries of its block.
#[derive(Debug, Clone)]
pub struct DeleteCategory {
    pub category_id: CategoryId,
}

/// Swaps a category with its predecessor in the order.
#[derive(Debug, Clone)]
pub struct MoveCategoryUp {
    pub category_id: CategoryId,
}

/// Swaps a category with its successor in the order.
#[derive(Debug, Clone)]
pub struct MoveCategoryDown {
    pub category_id: CategoryId,
}

/// What moving a category did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    Moved,
    /// There is no such category.
    NotFound,
    /// It is the first (moving up) or last (moving down) one already.
    AtEnd,
}

/// The write side of the category area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait CategoryCommands: Send + Sync {
    /// Gives the new ID and the warnings about gaps and overlaps of the
    /// categories' ages. `Rejected` carries the age rule it breaks;
    /// `NotFound` means there is no competition to derive the birthday
    /// bounds from.
    async fn add_category(
        &self,
        actor: &Actor,
        cmd: AddCategory,
    ) -> Result<(CategoryId, Vec<String>), CommandError>;

    /// Gives the age warnings. A category in a heat keeps its act type and
    /// gender (`Rejected` otherwise); `NotFound` for an unknown category.
    async fn edit_category(
        &self,
        actor: &Actor,
        cmd: EditCategory,
    ) -> Result<Vec<String>, CommandError>;

    /// `false` if there was no such category.
    async fn delete_category(
        &self,
        actor: &Actor,
        cmd: DeleteCategory,
    ) -> Result<bool, CommandError>;

    /// A refusal ([`MoveOutcome::NotFound`], [`MoveOutcome::AtEnd`]) changes
    /// nothing.
    async fn move_category_up(
        &self,
        actor: &Actor,
        cmd: MoveCategoryUp,
    ) -> Result<MoveOutcome, CommandError>;

    /// See [`CategoryCommands::move_category_up`].
    async fn move_category_down(
        &self,
        actor: &Actor,
        cmd: MoveCategoryDown,
    ) -> Result<MoveOutcome, CommandError>;
}
