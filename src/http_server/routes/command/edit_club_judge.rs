use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JudgeFields, JuryEnabled},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditClubJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditClubJudgeBody {
    judge_id: Uuid,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_club_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<EditClubJudgeBody>,
) -> Result<Json<EditClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let club_id = jury::ensure_judge_access(&mut transaction, &auth, body.judge_id).await?;
    let competition = Competition::current(&mut *transaction).await?;
    let fields = body
        .fields
        .normalize(competition.as_ref())
        .map_err(HttpError::ErrorMessages)?;
    info!("Editing judge {}", body.judge_id);
    jury::save_judge(&mut transaction, body.judge_id, club_id, &fields).await?;
    transaction.commit().await?;
    Ok(Json(EditClubJudgeResponse {}))
}
