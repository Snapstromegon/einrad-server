use crate::ids::JudgeId;
use axum::{Extension, Json, http::StatusCode};
use db_core::{commands::jury::EditClubJudge, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth, routes::http_types::JudgeFields},
    jury::JuryEnabled,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditClubJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditClubJudgeBody {
    judge_id: JudgeId,
    #[serde(flatten)]
    fields: JudgeFields,
}

/// Change a judge and replace their registrations. The club stays the same.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/edit_club_judge",
    request_body=EditClubJudgeBody,
    responses(
        (status=200, content_type="application/json", body=EditClubJudgeResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_club_judge(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<EditClubJudgeBody>,
) -> Result<Json<EditClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    info!("Editing judge {}", body.judge_id);
    repo.edit_club_judge(
        &auth.actor(),
        EditClubJudge {
            judge_id: body.judge_id,
            fields: body.fields.into(),
        },
    )
    .await?;
    Ok(Json(EditClubJudgeResponse {}))
}
