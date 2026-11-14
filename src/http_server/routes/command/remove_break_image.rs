use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::Slot,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RemoveBreakImageResponse {}

/// Remove the info board's break image.
///
/// The competition's logo applies again (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/remove_break_image",
    responses(
        (status=200, content_type="application/json", body=RemoveBreakImageResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn remove_break_image(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<RemoveBreakImageResponse>, HttpError> {
    repo.remove_image(Slot::Break).await?;
    info!("Removed the break image");
    Ok(Json(RemoveBreakImageResponse {}))
}
