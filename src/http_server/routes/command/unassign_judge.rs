use crate::ids::{JudgeId, JuryUnitId};
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
pub struct UnassignJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnassignJudgeBody {
    judge_id: JudgeId,
    unit_id: JuryUnitId,
}

/// Remove a judge from a jury unit of the Juryplan. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/unassign_judge",
    request_body=UnassignJudgeBody,
    responses(
        (status=200, content_type="application/json", body=UnassignJudgeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn unassign_judge(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<UnassignJudgeBody>,
) -> Result<Json<UnassignJudgeResponse>, HttpError> {
    info!(
        "Removing judge {} from jury unit {}",
        body.judge_id, body.unit_id
    );
    repo.unassign_judge(body.judge_id, body.unit_id).await?;
    Ok(Json(UnassignJudgeResponse {}))
}
