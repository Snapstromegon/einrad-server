use axum::{Extension, Json, http::StatusCode};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JudgeTask, JuryAssignment, JuryEnabled},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignJudgeResponse {}

/// Assign a judge a task in a jury category of the Juryplan. Untrained
/// people only count dismounts; one task per judge and category. Jury lead.
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn assign_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<JuryAssignment>,
) -> Result<Json<AssignJudgeResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let is_trained =
        sqlx::query_scalar!("SELECT is_trained FROM judge WHERE id = ?", body.judge_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if !is_trained && body.task != JudgeTask::Dismounts {
        return Err(HttpError::ErrorMessages(
            "Nicht geschulte Personen können nur Abstiege zählen.".to_string(),
        ));
    }
    info!(
        "Assigning judge {} to jury category {} ({:?})",
        body.judge_id, body.jury_category_id, body.task
    );
    let result = sqlx::query!(
        "INSERT INTO jury_assignments (judge_id, jury_category_id, task) VALUES (?, ?, ?)",
        body.judge_id,
        body.jury_category_id,
        body.task,
    )
    .execute(&mut *transaction)
    .await;
    match result {
        Ok(_) => {}
        Err(e) if jury::is_unique_violation(&e) => {
            return Err(HttpError::ErrorMessages(
                "Die JurorIn ist in dieser Jury-Kategorie schon eingeteilt.".to_string(),
            ));
        }
        Err(e)
            if e.as_database_error()
                .is_some_and(|e| e.is_foreign_key_violation()) =>
        {
            return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
        }
        Err(e) => return Err(e.into()),
    }
    transaction.commit().await?;
    Ok(Json(AssignJudgeResponse {}))
}
