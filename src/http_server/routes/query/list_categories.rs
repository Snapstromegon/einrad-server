use axum::{Extension, Json};
use tracing::instrument;

use crate::ids::{CategoryId, HeatId};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
};
use db_core::{prelude::*, queries::category as read};

/// A category as the API lists it.
#[derive(Debug, Serialize, ToSchema)]
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

impl From<read::Category> for Category {
    fn from(category: read::Category) -> Self {
        Self {
            id: category.id,
            name: category.name,
            description: category.description,
            min_age: category.min_age,
            under_age: category.under_age,
            act_type: category.act_type,
            is_single_male: category.is_single_male,
            order: category.order,
            einfahrzeit_seconds: category.einfahrzeit_seconds,
            act_duration_seconds: category.act_duration_seconds,
            judge_duration_seconds: category.judge_duration_seconds,
            heat_id: category.heat_id,
            heat: category.heat,
        }
    }
}

/// Get information about a club.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_categories",
    responses(
        (status=200, content_type="application/json", body=Vec<Category>),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_categories(
    Extension(repo): Extension<DbHandle>,
    auth: Option<Auth>,
) -> Result<Json<Vec<Category>>, HttpError> {
    let actor = auth.map(|auth| auth.actor());
    Ok(Json(
        repo.list_categories(actor.as_ref())
            .await?
            .into_iter()
            .map(Category::from)
            .collect(),
    ))
}
