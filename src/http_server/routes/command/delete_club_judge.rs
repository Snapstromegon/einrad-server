use crate::ids::JudgeId;
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::JuryEnabled,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteClubJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteClubJudgeBody {
    judge_id: JudgeId,
}

/// Remove a judge with their registrations.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/delete_club_judge",
    request_body=DeleteClubJudgeBody,
    responses(
        (status=200, content_type="application/json", body=DeleteClubJudgeResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_club_judge(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteClubJudgeBody>,
) -> Result<Json<DeleteClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    info!("Deleting judge {}", body.judge_id);
    repo.delete_judge(&auth.actor(), body.judge_id).await?;
    Ok(Json(DeleteClubJudgeResponse {}))
}
