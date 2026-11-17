use crate::ids::ActId;
use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    competition::Features,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditClubActResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditClubActBody {
    id: ActId,
    name: String,
    description: Option<String>,
}

/// Edit an act's title and description.
///
/// Only clubs with a rider in the act, the club that created a group, and
/// admins can edit it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_club_act",
    request_body=EditClubActBody,
    responses(
        (status=200, content_type="application/json", body=EditClubActResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_club_act(
    Extension(repo): Extension<DbHandle>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<EditClubActBody>,
) -> Result<Json<EditClubActResponse>, HttpError> {
    if !capabilities.can_upload_music {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    repo.ensure_act_access(&auth.actor(), body.id).await?;
    let moderation_input = Features::from_repo(repo.as_ref()).await?.moderation_input;
    repo.edit_act(
        body.id,
        &body.name,
        moderation_input,
        body.description.as_deref(),
    )
    .await?;

    Ok(Json(EditClubActResponse {}))
}
