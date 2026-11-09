use axum::{Extension, Json, extract::Multipart};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::{
    image_support::{read_upload, store_upload},
    set_competition_logo::LogoUpload,
};
use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::Slot,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetBreakImageResponse {}

/// Upload the info board's break image.
///
/// Shown while no act runs. Replaces an earlier break image or URL source
/// (#291). Checked and scaled like the logo. Without one, the competition's
/// logo applies. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/set_break_image",
    request_body=LogoUpload,
    responses(
        (status=200, content_type="application/json", body=SetBreakImageResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=413, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn set_break_image(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    body: Multipart,
) -> Result<Json<SetBreakImageResponse>, HttpError> {
    let (_, logo) = read_upload(body).await?;
    store_upload(&db, Slot::Break, &logo).await?;
    info!("Set the break image ({})", logo.format.name());
    Ok(Json(SetBreakImageResponse {}))
}
