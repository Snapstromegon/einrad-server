use crate::ids::{JudgeId, JurySessionId};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    jury_sessions,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct UnassignJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnassignJudgeBody {
    judge_id: JudgeId,
    session_id: JurySessionId,
}

/// Remove a judge from a Durchgang (jury session) of the Juryplan; their
/// other Durchgänge stay. Jury lead.
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn unassign_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<UnassignJudgeBody>,
) -> Result<Json<UnassignJudgeResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!(
        "Removing judge {} from jury session {}",
        body.judge_id, body.session_id
    );
    jury_sessions::unassign(&mut transaction, body.judge_id, body.session_id).await?;
    transaction.commit().await?;
    Ok(Json(UnassignJudgeResponse {}))
}
