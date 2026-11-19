use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    ids::{JudgeId, JurySessionId},
    jury::{JudgeTask, JuryEnabled},
    jury_sessions,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignJudgeBody {
    pub judge_id: JudgeId,
    /// The Durchgang; the unit is the one it belongs to.
    pub session_id: JurySessionId,
    pub task: JudgeTask,
}

/// Assign a judge a task in a Durchgang (jury session) of the Juryplan.
/// Untrained people only count dismounts; one task per judge and Durchgang,
/// but a judge may sit in several Durchgänge of a unit. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/assign_judge",
    request_body=AssignJudgeBody,
    responses(
        (status=200, content_type="application/json", body=AssignJudgeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn assign_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<AssignJudgeBody>,
) -> Result<Json<AssignJudgeResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!(
        "Assigning judge {} to jury session {} ({:?})",
        body.judge_id, body.session_id, body.task
    );
    jury_sessions::assign(&mut transaction, body.judge_id, body.session_id, body.task).await?;
    transaction.commit().await?;
    Ok(Json(AssignJudgeResponse {}))
}
