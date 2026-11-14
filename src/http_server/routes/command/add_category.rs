use crate::ids::CategoryId;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};
use db_core::{DbHandle, categories::CategoryFields};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddCategoryResponse {
    /// The new category's ID.
    category_id: CategoryId,
    /// Gaps and overlaps between the categories' birthday bounds, derived
    /// from the determining day, as the import reports them.
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddCategoryBody {
    name: String,
    description: Option<String>,
    /// The age on the determining day from which on riders belong to the
    /// category, e.g. 70 for "70+"; `None` for no lower age. Fractions are
    /// months: 9.5 is 9 years 6 months.
    min_age: Option<f64>,
    /// The age riders are under on the determining day, e.g. 11 for "U11";
    /// `None` for no upper age.
    under_age: Option<f64>,
    act_type: ActType,
    is_single_male: bool,
    einfahrzeit_seconds: i32,
    act_duration_seconds: i32,
    judge_duration_seconds: i32,
}

/// Add a new category, defined by its age class: the birthday bounds follow
/// from the determining day.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/add_category",
    request_body=AddCategoryBody,
    responses(
        (status=200, content_type="application/json", body=AddCategoryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn add_category(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddCategoryBody>,
) -> Result<Json<AddCategoryResponse>, HttpError> {
    let (category_id, warnings) = repo
        .add_category(&CategoryFields {
            name: body.name,
            description: body.description,
            min_age: body.min_age,
            under_age: body.under_age,
            act_type: body.act_type,
            is_single_male: body.is_single_male,
            einfahrzeit_seconds: body.einfahrzeit_seconds,
            act_duration_seconds: body.act_duration_seconds,
            judge_duration_seconds: body.judge_duration_seconds,
        })
        .await?;
    Ok(Json(AddCategoryResponse {
        category_id,
        warnings,
    }))
}
