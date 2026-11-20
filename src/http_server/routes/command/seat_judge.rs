use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    ids::{JudgeId, JurySessionId},
    jury::{JudgeTask, JuryEnabled},
    jury_sessions::{self, SeatOutcome},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SeatJudgeResponse {
    /// What happened: assigned, moved, swapped, moved_replaced, replaced or
    /// unchanged.
    pub outcome: SeatOutcome,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SeatJudgeBody {
    pub judge_id: JudgeId,
    /// The Durchgang; the unit is the one it belongs to.
    pub session_id: JurySessionId,
    pub task: JudgeTask,
    /// The seat (T1.., P1.., A1..) within the Durchgang's needed count.
    pub seat: i64,
}

/// Put a judge on a seat of a Durchgang (jury session): assigns the judge,
/// or, if the judge already sits elsewhere in the Durchgang, moves them; an
/// occupant of the seat swaps with the judge when they can take the judge's
/// old seat, else is removed; a judge without a seat replaces the occupant.
/// All in one transaction; the outcome says which happened. Seats beyond the
/// Durchgang's needed count can't be chosen. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/seat_judge",
    request_body=SeatJudgeBody,
    responses(
        (status=200, content_type="application/json", body=SeatJudgeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn seat_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SeatJudgeBody>,
) -> Result<Json<SeatJudgeResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!(
        "Seating judge {} in jury session {} on {:?} {}",
        body.judge_id, body.session_id, body.task, body.seat
    );
    let outcome = jury_sessions::seat_judge(
        &mut transaction,
        body.judge_id,
        body.session_id,
        body.task,
        body.seat,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(SeatJudgeResponse { outcome }))
}
