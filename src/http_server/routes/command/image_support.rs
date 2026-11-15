//! What the commands on the competition's images (#286, #291) share.

use axum::{extract::Multipart, http::StatusCode};
use db_core::{DbHandle, commands::media::ImageInput};

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

/// The new source of an image from an inline SVG or a URL (exactly one of
/// them), checked without any request. A URL is downloaded afterwards, see
/// [`fetch_if_url`].
pub fn image_input(svg: Option<&str>, url: Option<&str>) -> Result<ImageInput, HttpError> {
    match (
        svg.filter(|s| !s.trim().is_empty()),
        url.filter(|u| !u.trim().is_empty()),
    ) {
        (Some(svg), None) => Ok(ImageInput::Data(svg_logo(svg)?)),
        (None, Some(url)) => Ok(ImageInput::Url(image_url(url)?.to_string())),
        _ => Err(HttpError::ErrorMessages(
            "Bitte entweder ein SVG oder eine URL angeben.".to_string(),
        )),
    }
}

/// Downloads a URL source right away, so the admin sees at once if it works;
/// a failed download isn't an error here, it's the image's state.
pub async fn fetch_if_url(
    repo: &DbHandle,
    fetcher: &ImageFetcher,
    slot: Slot,
    input: &ImageInput,
) -> Result<(), HttpError> {
    if matches!(input, ImageInput::Url(_)) {
        fetcher.refresh(repo, Some(slot)).await?;
    }
    Ok(())
}
