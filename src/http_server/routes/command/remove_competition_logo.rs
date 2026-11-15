use axum::{Extension, Json};
use db_core::prelude::*;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RemoveCompetitionLogoResponse {}

/// Remove the competition's logo.
///
/// The logo built into the binary applies again (#286); a URL source goes too. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/remove_competition_logo",
    responses(
        (status=200, content_type="application/json", body=RemoveCompetitionLogoResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn remove_competition_logo(
    Extension(repo): Extension<DbHandle>,
    auth: Granted<permit::ManageCompetition>,
) -> Result<Json<RemoveCompetitionLogoResponse>, HttpError> {
    repo.remove_competition_logo(&auth.actor()).await?;
    info!("Removed the competition logo");
    Ok(Json(RemoveCompetitionLogoResponse {}))
}
