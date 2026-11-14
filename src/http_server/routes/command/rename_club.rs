use crate::ids::ClubId;
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{HttpError, extractor::auth::Auth},
    roles::Permission,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RenameClubResponse {
    club_id: ClubId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameClubBody {
    club_id: ClubId,
    name: String,
}

/// Rename a club.
///
/// This endpoint is used to rename a club.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/rename_club",
    request_body=RenameClubBody,
    responses(
        (status=200, content_type="application/json", body=RenameClubResponse),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn rename_club(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<RenameClubBody>,
) -> Result<Json<RenameClubResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let only_owner = (!auth.can(Permission::ManageRegistrations)).then_some(auth.user_id);
    db.rename_club(body.club_id, &body.name, only_owner)
        .await
        .map_err(|e| {
            tracing::error!("Failed to rename club: {:?}", e);
            HttpError::InternalServerError
        })?;

    Ok(Json(RenameClubResponse {
        club_id: body.club_id,
    }))
}
