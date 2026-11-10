use std::sync::Arc;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::image_support::apply_source;
use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    image_fetch::ImageFetcher,
    images::{CompetitionImages, Slot},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageTarget {
    Logo,
    BreakImage,
    /// With `sponsor_id`.
    Sponsor,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetImageSourceBody {
    pub target: ImageTarget,
    /// The sponsor, for the target `sponsor`.
    pub sponsor_id: Option<i64>,
    /// An inline SVG; or
    pub svg: Option<String>,
    /// a URL the server downloads the image from.
    pub url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SetImageSourceResponse {
    /// The images after the change, with the result of a download.
    pub images: CompetitionImages,
}

/// Set the source of the logo, the break image or a sponsor's logo to an
/// inline SVG or a URL.
///
/// An SVG gets the checks of an upload. A URL is downloaded at once and again
/// at every start and by the refresh; the server serves the local copy and
/// never hands the URL to a browser (#291). A failed download isn't an error:
/// it shows in the response and the image keeps its last good copy. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/set_image_source",
    request_body=SetImageSourceBody,
    responses(
        (status=200, content_type="application/json", body=SetImageSourceResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, fetcher, body))]
#[axum::debug_handler]
pub async fn set_image_source(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(fetcher): Extension<Arc<ImageFetcher>>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<SetImageSourceBody>,
) -> Result<Json<SetImageSourceResponse>, HttpError> {
    let slot = match (body.target, body.sponsor_id) {
        (ImageTarget::Logo, _) => Slot::Logo,
        (ImageTarget::BreakImage, _) => Slot::Break,
        (ImageTarget::Sponsor, Some(id)) => Slot::Sponsor(id),
        (ImageTarget::Sponsor, None) => {
            return Err(HttpError::ErrorMessages("Der Sponsor fehlt.".to_string()));
        }
    };
    apply_source(
        &db,
        &fetcher,
        slot,
        body.svg.as_deref(),
        body.url.as_deref(),
    )
    .await?;
    info!("Set the source of {slot:?}");
    let pool = db.get().await.clone();
    Ok(Json(SetImageSourceResponse {
        images: CompetitionImages::load(&pool).await?,
    }))
}
