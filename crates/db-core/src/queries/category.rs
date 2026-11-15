//! Category queries: categories.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use crate::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{CategoryId, HeatId},
};

/// A category as the API lists it.
#[derive(Debug, Clone, PartialEq)]
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

/// The read side of the category area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait CategoryQueries: Send + Sync {
    /// The categories in order, those without an order first. Open to
    /// anonymous callers.
    async fn list_categories(&self, actor: Option<&Actor>) -> Result<Vec<Category>, QueryError>;
}
