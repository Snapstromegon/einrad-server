use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::ensure_act_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DissolvePairResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DissolvePairBody {
    act_id: Uuid,
}

/// Dissolves a pair act: both riders leave the pair competition, nobody is
/// left waiting for a partner. To keep the partner waiting, use `leave_pair`.
///
/// Only the club of one of its riders (or an admin) can do it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/dissolve_pair",
    request_body=DissolvePairBody,
    responses(
        (status=200, content_type="application/json", body=DissolvePairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn dissolve_pair(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DissolvePairBody>,
) -> Result<Json<DissolvePairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_act_access(&mut transaction, &auth, body.act_id).await?;
    rider_registration::dissolve_pair(&mut transaction, body.act_id).await?;
    transaction.commit().await?;
    Ok(Json(DissolvePairResponse {}))
}
