use crate::ids::{JudgeId, JuryUnitId};
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryEnabled, JuryUnitKind},
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
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn move_judge_registrations(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<MoveJudgeRegistrationsBody>,
) -> Result<Json<MoveJudgeRegistrationsResponse>, HttpError> {
    info!(
        "Moving the registrations of judge {} from {:?} {} to {:?} {}",
        body.judge_id, body.from_kind, body.from_id, body.to_kind, body.to_id
    );
    repo.move_judge_registrations(body.judge_id, body.from_id, body.to_kind, body.to_id)
        .await?;
    Ok(Json(MoveJudgeRegistrationsResponse {}))
}
