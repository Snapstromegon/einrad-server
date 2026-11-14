use crate::ids::{ClubId, JudgeId};
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{JudgeFields, JuryEnabled},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddClubJudgeResponse {
    judge_id: JudgeId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddClubJudgeBody {
    club_id: ClubId,
    #[serde(flatten)]
    fields: JudgeFields,
}

/// Register a judge of a club, with the jury units (heats' Vorlaufs and finals) and tasks they can
/// do. Clubs can until the judge deadline, admins always.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/add_club_judge",
    request_body=AddClubJudgeBody,
    responses(
        (status=200, content_type="application/json", body=AddClubJudgeResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn add_club_judge(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<AddClubJudgeBody>,
) -> Result<Json<AddClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    info!("Adding a judge to club {}", body.club_id);
    let judge_id = repo
        .add_judge(&auth.actor(), body.club_id, body.fields)
        .await?;
    Ok(Json(AddClubJudgeResponse { judge_id }))
}
