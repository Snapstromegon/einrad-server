//! Categories: the [`CategoriesRepo`] trait.

use crate::{
    DbError,
    act_type::ActType,
    clubs::RegistrationError,
    ids::{CategoryId, HeatId},
};

/// A category as the API lists it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub description: Option<String>,
    /// The age on the determining day from which on riders belong to the
    /// category; `None` for no lower age. Fractions are months.
    pub min_age: Option<f64>,
    /// The age riders are under on the determining day; `None` for no
    /// upper age.
    pub under_age: Option<f64>,
    pub act_type: ActType,
    pub is_single_male: bool,
    pub order: Option<i64>,
    pub einfahrzeit_seconds: Option<i64>,
    pub act_duration_seconds: Option<i64>,
    pub judge_duration_seconds: Option<i64>,
    /// The heat it belongs to, if any, by id; clubs never pick it.
    pub heat_id: Option<HeatId>,
    /// The name of its heat.
    pub heat: Option<String>,
}

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

/// Which way to move a category in the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
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

#[async_trait::async_trait]
pub trait CategoriesRepo {
    /// The categories in order, those without an order first.
    async fn list_categories(&self) -> Result<Vec<Category>, DbError>;

    /// Adds a category at the end of the order; gives its ID and the
    /// warnings about gaps and overlaps of the categories' ages. A
    /// `Rejected` error carries the age rule it breaks; `NotFound` means
    /// there is no competition to derive the birthday bounds from.
    async fn add_category(
        &self,
        fields: &CategoryFields,
    ) -> Result<(CategoryId, Vec<String>), RegistrationError>;

    /// Overwrites a category (a rename included); gives the age warnings.
    /// A category in a heat keeps its act type and gender.
    async fn edit_category(
        &self,
        id: CategoryId,
        fields: &CategoryFields,
    ) -> Result<Vec<String>, RegistrationError>;

    /// Deletes a category with the timeplan entries of its block; `false`
    /// if there was none.
    async fn delete_category(&self, id: CategoryId) -> Result<bool, DbError>;

    /// Swaps a category with its neighbour in the order.
    async fn move_category(
        &self,
        id: CategoryId,
        direction: MoveDirection,
    ) -> Result<MoveOutcome, DbError>;
}
