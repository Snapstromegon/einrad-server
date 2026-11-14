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
pub struct DeleteClubGroupResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteClubGroupBody {
    act_id: ActId,
}

/// Delete a group. Its riders stay registered.
///
/// Only the group's owner club (or an admin) can delete it. The owner club
/// can't delete a group while riders of other clubs are still in it.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/delete_club_group",
    request_body=DeleteClubGroupBody,
    responses(
        (status=200, content_type="application/json", body=DeleteClubGroupResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_club_group(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteClubGroupBody>,
) -> Result<Json<DeleteClubGroupResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    db.delete_group(&auth.actor(), body.act_id).await?;

    Ok(Json(DeleteClubGroupResponse {}))
}
