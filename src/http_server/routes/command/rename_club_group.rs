use crate::ids::ActId;
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RenameClubGroupResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameClubGroupBody {
    act_id: ActId,
    group_name: String,
}

/// Rename a group.
///
/// Only the group's owner club (or an admin) can rename it.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/rename_club_group",
    request_body=RenameClubGroupBody,
    responses(
        (status=200, content_type="application/json", body=RenameClubGroupResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn rename_club_group(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<RenameClubGroupBody>,
) -> Result<Json<RenameClubGroupResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.rename_group(&auth.actor(), body.act_id, &body.group_name)
        .await?;

    Ok(Json(RenameClubGroupResponse {}))
}
