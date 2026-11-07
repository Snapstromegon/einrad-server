use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct UnassignJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnassignJudgeBody {
    judge_id: Uuid,
    jury_category_id: Uuid,
}

/// Remove a judge from a jury category of the Juryplan. Jury lead.
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
        "Removing judge {} from jury category {}",
        body.judge_id, body.jury_category_id
    );
    sqlx::query!(
        "DELETE FROM jury_assignments WHERE judge_id = ? AND jury_category_id = ?",
        body.judge_id,
        body.jury_category_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(UnassignJudgeResponse {}))
}
