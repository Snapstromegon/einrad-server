use serde::{Deserialize, Serialize};

/// A rider's gender. It decides whether a single counts as Einzel weiblich
/// or männlich. Stored as text in `starter.gender`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum Gender {
    Female,
    Male,
}
