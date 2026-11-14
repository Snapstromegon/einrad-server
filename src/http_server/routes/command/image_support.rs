//! What the commands on the competition's images (#286, #291) share.

use axum::{extract::Multipart, http::StatusCode};
use db_core::DbHandle;

use crate::{
    http_server::HttpError,
    image_fetch::ImageFetcher,
    images::{self, ImageUrl, Slot},
    logo::{self, Logo, SvgLogo},
};

/// Reads a multipart upload: its text fields as `(name, value)` before the
/// file, which is the last field, and the checked image. A body over the
/// request limit fails as 413.
pub async fn read_upload(mut body: Multipart) -> Result<(Vec<(String, String)>, Logo), HttpError> {
    let mut fields = Vec::new();
    let bytes = loop {
        let field = body
            .next_field()
            .await
            .map_err(|e| HttpError::StatusCode(e.status()))?
            .ok_or(HttpError::StatusCode(StatusCode::BAD_REQUEST))?;
        if field.file_name().is_some() || field.name() == Some("file") {
            break field
                .bytes()
                .await
                .map_err(|e| HttpError::StatusCode(e.status()))?;
        }
        let name = field.name().unwrap_or_default().to_string();
        let value = field
            .text()
            .await
            .map_err(|e| HttpError::StatusCode(e.status()))?;
        fields.push((name, value));
    };
    // Scaling a raster image takes a moment.
    let logo = tokio::task::spawn_blocking(move || logo::from_upload(&bytes))
        .await
        .map_err(|_| HttpError::InternalServerError)?
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    Ok((fields, logo))
}

/// Stores an uploaded image in `slot`.
pub async fn store_upload(repo: &DbHandle, slot: Slot, logo: &Logo) -> Result<(), HttpError> {
    if !repo.set_image_data(slot, logo).await? {
        return Err(HttpError::NotFound);
    }
    Ok(())
}

/// An inline SVG, checked like an upload.
pub fn svg_logo(svg: &str) -> Result<Logo, HttpError> {
    svg.parse::<SvgLogo>()
        .map(|svg| Logo::from(&svg))
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))
}

/// A URL to download an image from, checked without a request.
pub fn image_url(url: &str) -> Result<ImageUrl, HttpError> {
    url.parse()
        .map_err(|e: images::UrlError| HttpError::ErrorMessages(e.to_string()))
}

/// Sets the image in `slot` from an inline SVG or a URL (exactly one of
/// them). A URL is downloaded right away, so the admin sees at once if it
/// works; a failed download isn't an error here, it's the image's state.
pub async fn apply_source(
    repo: &DbHandle,
    fetcher: &ImageFetcher,
    slot: Slot,
    svg: Option<&str>,
    url: Option<&str>,
) -> Result<(), HttpError> {
    match (
        svg.filter(|s| !s.trim().is_empty()),
        url.filter(|u| !u.trim().is_empty()),
    ) {
        (Some(svg), None) => store_upload(repo, slot, &svg_logo(svg)?).await,
        (None, Some(url)) => {
            let url = image_url(url)?;
            if !repo.set_image_url(slot, &url.to_string()).await? {
                return Err(HttpError::NotFound);
            }
            fetcher.refresh(repo, Some(slot)).await?;
            Ok(())
        }
        _ => Err(HttpError::ErrorMessages(
            "Bitte entweder ein SVG oder eine URL angeben.".to_string(),
        )),
    }
}
