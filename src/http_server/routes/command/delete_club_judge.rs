use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JuryEnabled},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteClubJudgeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteClubJudgeBody {
    judge_id: Uuid,
}

/// Remove a judge with their registrations.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/delete_club_judge",
    request_body=DeleteClubJudgeBody,
    responses(
        (status=200, content_type="application/json", body=DeleteClubJudgeResponse),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_club_judge(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteClubJudgeBody>,
) -> Result<Json<DeleteClubJudgeResponse>, HttpError> {
    if !capabilities.can_register_judge {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    jury::ensure_judge_access(&mut transaction, &auth, body.judge_id).await?;
    info!("Deleting judge {}", body.judge_id);
    // The registrations go with it (foreign key).
    sqlx::query!("DELETE FROM judge WHERE id = ?", body.judge_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Json(DeleteClubJudgeResponse {}))
}
