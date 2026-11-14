use crate::ids::ClubId;
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
pub struct CreateClubResponse {
    club_id: ClubId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClubBody {
    name: String,
}

/// CreateClub a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/create_club",
    request_body=CreateClubBody,
    responses(
        (status=200, content_type="application/json", body=CreateClubResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn create_club(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<CreateClubBody>,
) -> Result<Json<CreateClubResponse>, HttpError> {
    if !capabilities.can_create_club {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let club_id = db.create_club(auth.user_id, &body.name).await?;

    Ok(Json(CreateClubResponse { club_id }))
}
