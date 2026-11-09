use axum::{Extension, Json, extract::Multipart};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::image_support::{read_upload, store_upload};
use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::Slot,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetCompetitionLogoResponse {}

/// The multipart body: the image as its only file.
#[derive(ToSchema, Debug)]
pub struct LogoUpload {
    #[allow(dead_code)]
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

/// Upload the competition's logo.
///
/// Replaces an earlier logo or URL source (#286). The format comes from the
/// content: PNG, JPEG, WebP or SVG, up to 1 MiB. Raster images are scaled
/// down to 512 px on the long edge and lose their metadata; SVG images are
/// stored as they are, but refused if they could run scripts or load other
/// files (see `crate::logo`). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/set_competition_logo",
    request_body=LogoUpload,
    responses(
        (status=200, content_type="application/json", body=SetCompetitionLogoResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=413, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn set_competition_logo(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    body: Multipart,
) -> Result<Json<SetCompetitionLogoResponse>, HttpError> {
    let (_, logo) = read_upload(body).await?;
    store_upload(&db, Slot::Logo, &logo).await?;
    info!("Set the competition logo ({})", logo.format.name());
    Ok(Json(SetCompetitionLogoResponse {}))
}
