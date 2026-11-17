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
pub struct EditCategoryResponse {
    /// Gaps and overlaps between the categories' birthday bounds, derived
    /// from the determining day, as the import reports them.
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditCategoryBody {
    /// The category to edit.
    category_id: CategoryId,
    new_name: String,
    description: Option<String>,
    /// See `AddCategoryBody::min_age`.
    min_age: Option<f64>,
    /// See `AddCategoryBody::under_age`.
    under_age: Option<f64>,
    act_type: ActType,
    is_single_male: bool,
    einfahrzeit_seconds: i32,
    act_duration_seconds: i32,
    judge_duration_seconds: i32,
}

/// Edit an existing category.
///
/// This endpoint is used to update a category's properties.
#[utoipa::path(
    post,
    tags=["command", "category"],
    path="/edit_category",
    request_body=EditCategoryBody,
    responses(
        (status=200, content_type="application/json", body=EditCategoryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_category(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditCategoryBody>,
) -> Result<Json<EditCategoryResponse>, HttpError> {
    let warnings = repo
        .edit_category(
            body.category_id,
            &CategoryFields {
                name: body.new_name,
                description: body.description,
                min_age: body.min_age,
                under_age: body.under_age,
                act_type: body.act_type,
                is_single_male: body.is_single_male,
                einfahrzeit_seconds: body.einfahrzeit_seconds,
                act_duration_seconds: body.act_duration_seconds,
                judge_duration_seconds: body.judge_duration_seconds,
            },
        )
        .await?;
    Ok(Json(EditCategoryResponse { warnings }))
}
