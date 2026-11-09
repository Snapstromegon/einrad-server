use axum::{Extension, Json};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::{self, Slot},
    reloadable_sqlite::ReloadableSqlite,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn remove_competition_logo(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<RemoveCompetitionLogoResponse>, HttpError> {
    let db = db.get().await.clone();
    images::remove(&db, Slot::Logo).await?;
    info!("Removed the competition logo");
    Ok(Json(RemoveCompetitionLogoResponse {}))
}
