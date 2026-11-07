//! The info board's photos as images: `/api/photos/newest` and
//! `/api/photos/random` redirect to `/api/photos/{id}`, which never changes.

use std::sync::Arc;

use axum::{
    Extension,
    extract::Path,
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION},
    },
    response::{IntoResponse, Response},
};
use tracing::warn;

use crate::photos::{PhotoInfo, Photos};

fn redirect(photo: Option<PhotoInfo>) -> Response {
    match photo {
        Some(photo) => (
            StatusCode::FOUND,
            [(LOCATION, photo.url), (CACHE_CONTROL, "no-store".into())],
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, [(CACHE_CONTROL, "no-store")]).into_response(),
    }
}

/// The newest photo (after an act).
pub async fn newest(Extension(photos): Extension<Arc<Photos>>) -> Response {
    redirect(photos.newest())
}

/// One of the recent photos (in breaks).
pub async fn random(Extension(photos): Extension<Arc<Photos>>) -> Response {
    let recent = photos.recent();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or_default();
    redirect((!recent.is_empty()).then(|| recent[nanos % recent.len()].clone()))
}

pub async fn image(Extension(photos): Extension<Arc<Photos>>, Path(id): Path<String>) -> Response {
    match photos.image(&id).await {
        Ok(Some(image)) => (
            [
                (CONTENT_TYPE, image.content_type),
                // The id changes with the file.
                (CACHE_CONTROL, "public, max-age=31536000, immutable".into()),
            ],
            image.bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!("Couldn't load photo {id}: {e}");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}
