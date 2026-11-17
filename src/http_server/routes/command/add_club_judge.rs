use crate::ids::{ClubId, JudgeId, RiderId};
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    competition::Competition,
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JudgeFields, JuryEnabled},
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
    /// Creates the judge from this rider of the club: name and birthdate are
    /// the rider's then, and any sent here are ignored.
    #[serde(default)]
    rider_id: Option<RiderId>,
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
        (status=400, content_type="application/json", body=ClientError),
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
    let identity = jury::resolve_identity(
        &mut transaction,
        None,
        body.club_id,
        body.rider_id,
        &body.fields,
    )
    .await?;
    let fields = body
        .fields
        .normalize(competition.as_ref(), &identity)
        .map_err(HttpError::ErrorMessages)?;
    let judge_id = JudgeId::new();
    info!("Adding judge {judge_id} to club {}", body.club_id);
    jury::save_judge(
        &mut transaction,
        judge_id,
        body.club_id,
        body.rider_id,
        &identity,
        &fields,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(AddClubJudgeResponse { judge_id }))
}
