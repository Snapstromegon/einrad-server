use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    image_fetch::ImageFetcher,
    images::CompetitionImages,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RefreshImagesResponse {
    /// The images after the downloads: a failed one has its `fetch_error`,
    /// and keeps its last good copy.
    pub images: CompetitionImages,
}

/// Download all URL images again ("Bilder aktualisieren").
///
/// A failed download keeps the last good copy and is reported in the
/// response and in the competition management (#291). Takes as long as the
/// slowest download, up to 30 seconds. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/refresh_images",
    responses(
        (status=200, content_type="application/json", body=RefreshImagesResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, fetcher))]
#[axum::debug_handler]
pub async fn refresh_images(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(fetcher): Extension<Arc<ImageFetcher>>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<RefreshImagesResponse>, HttpError> {
    fetcher.refresh(&db, None).await?;
    info!("Refreshed the images");
    let pool = db.get().await.clone();
    Ok(Json(RefreshImagesResponse {
        images: CompetitionImages::load(&pool).await?,
    }))
}
