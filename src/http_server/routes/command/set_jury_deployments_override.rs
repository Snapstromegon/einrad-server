use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetJuryDeploymentsOverrideResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJuryDeploymentsOverrideBody {
    club_id: Uuid,
    /// The deployments to credit the club with; null to use the registered
    /// ones again.
    deployments: Option<i64>,
}

/// Credit a club with jury deployments instead of the registered ones, e.g.
/// after a late cancellation or as a waiver. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_deployments_override",
    request_body=SetJuryDeploymentsOverrideBody,
    responses(
        (status=200, content_type="application/json", body=SetJuryDeploymentsOverrideResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_jury_deployments_override(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SetJuryDeploymentsOverrideBody>,
) -> Result<Json<SetJuryDeploymentsOverrideResponse>, HttpError> {
    if body.deployments.is_some_and(|d| d < 0) {
        return Err(HttpError::ErrorMessages(
            "Die angerechneten Einsätze dürfen nicht negativ sein.".to_string(),
        ));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!(
        "Crediting club {} with {:?} jury deployments",
        body.club_id, body.deployments
    );
    let result = sqlx::query!(
        "UPDATE clubs SET jury_deployments_override = ? WHERE id = ?",
        body.deployments,
        body.club_id
    )
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    transaction.commit().await?;
    Ok(Json(SetJuryDeploymentsOverrideResponse {}))
}
