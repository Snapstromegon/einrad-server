use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::ensure_starter_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteClubStarterResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteClubStarterBody {
    starter_id: Uuid,
}

/// Delete a starter.
///
/// Only the starter's club (or an admin) can delete them. Their single and
/// pair acts are deleted too; the partner is left with a pair request. Groups
/// keep their other riders.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/delete_club_starter",
    request_body=DeleteClubStarterBody,
    responses(
        (status=200, content_type="application/json", body=DeleteClubStarterResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_club_starter(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteClubStarterBody>,
) -> Result<Json<DeleteClubStarterResponse>, HttpError> {
    if !capabilities.can_register_starter {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_starter_access(&mut transaction, &auth, body.starter_id).await?;

    rider_registration::delete_rider(&mut transaction, body.starter_id).await?;

    transaction.commit().await?;

    Ok(Json(DeleteClubStarterResponse {}))
}
