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
pub struct UploadSponsorImageResponse {}

/// The multipart body: the sponsor's id, then the image as the file.
#[derive(ToSchema, Debug)]
pub struct SponsorImageUpload {
    #[allow(dead_code)]
    pub id: i64,
    #[allow(dead_code)]
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

/// Upload a sponsor's logo.
///
/// Replaces an earlier image or URL source (#291). Checked and scaled like
/// the logo. The text field `id` comes before the file. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/upload_sponsor_image",
    request_body=SponsorImageUpload,
    responses(
        (status=200, content_type="application/json", body=UploadSponsorImageResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=413, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn upload_sponsor_image(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    body: Multipart,
) -> Result<Json<UploadSponsorImageResponse>, HttpError> {
    let (fields, logo) = read_upload(body).await?;
    let id = fields
        .iter()
        .find(|(name, _)| name == "id")
        .and_then(|(_, value)| value.trim().parse::<i64>().ok())
        .ok_or_else(|| HttpError::ErrorMessages("Der Sponsor fehlt.".to_string()))?;
    store_upload(&db, Slot::Sponsor(id), &logo).await?;
    info!("Set the image of sponsor {id} ({})", logo.format.name());
    Ok(Json(UploadSponsorImageResponse {}))
}
