use crate::ids::{JudgeId, JuryUnitId};
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JuryEnabled, JuryUnitKind},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct MoveJudgeRegistrationsResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveJudgeRegistrationsBody {
    judge_id: JudgeId,
    from_kind: JuryUnitKind,
    from_id: JuryUnitId,
    to_kind: JuryUnitKind,
    to_id: JuryUnitId,
}

/// Move all registrations of a judge for one jury unit to another, e.g.
/// from a skipped Vorlauf. A registration the judge already has there
/// merges (at most one per judge, task and unit). Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/move_judge_registrations",
    request_body=MoveJudgeRegistrationsBody,
    responses(
        (status=200, content_type="application/json", body=MoveJudgeRegistrationsResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn move_judge_registrations(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<MoveJudgeRegistrationsBody>,
) -> Result<Json<MoveJudgeRegistrationsResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    match jury::unit_is_registrable(&mut transaction, body.to_kind, body.to_id).await? {
        None => return Err(HttpError::StatusCode(StatusCode::NOT_FOUND)),
        Some(false) if body.from_id != body.to_id => return Err(jury::not_registrable()),
        Some(_) => {}
    }
    let (to_heat, to_final) = jury::unit_columns(body.to_kind, body.to_id);
    info!(
        "Moving the registrations of judge {} from {:?} {} to {:?} {}",
        body.judge_id, body.from_kind, body.from_id, body.to_kind, body.to_id
    );
    if body.from_id == body.to_id {
        return Ok(Json(MoveJudgeRegistrationsResponse {}));
    }
    sqlx::query!(
        r#"
        INSERT INTO judge_registration (judge_id, heat_id, final_id, task)
        SELECT judge_id, $1, $2, task FROM judge_registration
        WHERE judge_id = $3 AND unit_id = $4
        ON CONFLICT DO NOTHING
        "#,
        to_heat,
        to_final,
        body.judge_id,
        body.from_id,
    )
    .execute(&mut *transaction)
    .await?;
    let result = sqlx::query!(
        "DELETE FROM judge_registration WHERE judge_id = $1 AND unit_id = $2",
        body.judge_id,
        body.from_id,
    )
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    transaction.commit().await?;
    Ok(Json(MoveJudgeRegistrationsResponse {}))
}
