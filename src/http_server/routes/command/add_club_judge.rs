use crate::ids::{ClubId, JudgeId};
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JudgeFields, JuryEnabled, ensure_club_access},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_club_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<AddClubJudgeBody>,
) -> Result<Json<AddClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_club_access(&mut transaction, &auth, body.club_id).await?;
    let competition = Competition::current(&mut *transaction).await?;
    let fields = body
        .fields
        .normalize(competition.as_ref())
        .map_err(HttpError::ErrorMessages)?;
    let judge_id = JudgeId::new();
    info!("Adding judge {judge_id} to club {}", body.club_id);
    jury::save_judge(&mut transaction, judge_id, body.club_id, &fields).await?;
    transaction.commit().await?;
    Ok(Json(AddClubJudgeResponse { judge_id }))
}
