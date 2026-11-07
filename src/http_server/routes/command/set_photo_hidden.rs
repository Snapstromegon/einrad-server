use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    photos::Photos,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetPhotoHiddenResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PhotoHiddenBody {
    id: String,
    hidden: bool,
}

/// Hides a photo from the info board (e.g. a blurry one), so the next newest
/// shows instead, or shows it again. Kept until the server restarts.
#[utoipa::path(
    post,
    tags=["command", "photos"],
    path="/set_photo_hidden",
    request_body=PhotoHiddenBody,
    responses(
        (status=200, content_type="application/json", body=SetPhotoHiddenResponse),
        (status=404, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(photos))]
#[axum::debug_handler]
pub async fn set_photo_hidden(
    Extension(photos): Extension<Arc<Photos>>,
    _: Granted<permit::ControlMusic>,
    Json(body): Json<PhotoHiddenBody>,
) -> Result<Json<SetPhotoHiddenResponse>, HttpError> {
    if !photos.set_hidden(&body.id, body.hidden) {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    info!("Photo {} hidden: {}", body.id, body.hidden);
    Ok(Json(SetPhotoHiddenResponse {}))
}
