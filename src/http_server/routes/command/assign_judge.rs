use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryAssignment, JuryEnabled},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignJudgeResponse {}

/// Assign a judge a task in a jury unit of the Juryplan. Untrained
/// people only count dismounts; one task per judge and unit. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/assign_judge",
    request_body=JuryAssignment,
    responses(
        (status=200, content_type="application/json", body=AssignJudgeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn assign_judge(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<JuryAssignment>,
) -> Result<Json<AssignJudgeResponse>, HttpError> {
    info!(
        "Assigning judge {} to jury unit {:?} {} ({:?})",
        body.judge_id, body.unit_kind, body.unit_id, body.task
    );
    repo.assign_judge(&body).await?;
    Ok(Json(AssignJudgeResponse {}))
}
