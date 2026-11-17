use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetJuryPlanPublishedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJuryPlanPublishedBody {
    published: bool,
}

/// Publish the Juryplan to the clubs, or take it back. Each club then sees
/// its own judges' assignments. Publishing waits for the end of the judge
/// registration, so clubs can't change their judges under the plan. Admin
/// only.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_plan_published",
    request_body=SetJuryPlanPublishedBody,
    responses(
        (status=200, content_type="application/json", body=SetJuryPlanPublishedResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn set_jury_plan_published(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SetJuryPlanPublishedBody>,
) -> Result<Json<SetJuryPlanPublishedResponse>, HttpError> {
    info!("Juryplan published: {}", body.published);
    repo.set_jury_plan_published(body.published, time::OffsetDateTime::now_utc())
        .await?;
    Ok(Json(SetJuryPlanPublishedResponse {}))
}
