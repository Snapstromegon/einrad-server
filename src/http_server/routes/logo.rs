//! The competition's images (see `crate::images`): the logo at
//! `/api/competition/logo`, for every page and the favicon, the info board's
//! break image and the sponsor logos. The URL never changes, so browsers ask again
//! each time (`no-cache`) and mostly get a 304 by the ETag, which only reads
//! the logo's `updated_at`.

use axum::{
    Extension,
    extract::Path,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE,
            LAST_MODIFIED, X_CONTENT_TYPE_OPTIONS,
        },
    },
    response::{IntoResponse, Response},
};
use db_core::{DbError, DbHandle};
use time::{
    OffsetDateTime, PrimitiveDateTime, UtcOffset, format_description::BorrowedFormatItem, macros,
};
use tracing::warn;

use super::etag::{fnv1a, if_none_match};
use crate::{
    images::{self, Slot},
    logo::{self, Format},
};

/// Opened on its own, an SVG logo runs no scripts and loads nothing
/// (ADR 0009).
const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox";

/// The HTTP date format (RFC 9110, IMF-fixdate).
const HTTP_DATE: &[BorrowedFormatItem<'_>] = macros::format_description!(
    "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
);

/// So a default's ETag changes with the image.
const DEFAULT_LOGO_HASH: u64 = fnv1a(logo::DEFAULT_SVG);
const DEFAULT_BREAK_HASH: u64 = fnv1a(images::DEFAULT_BREAK_SVG);

/// The competition's logo, or the built-in default without one.
pub async fn competition_logo(
    Extension(repo): Extension<DbHandle>,
    request: HeaderMap,
) -> Response {
    let default = Some((logo::DEFAULT_SVG, DEFAULT_LOGO_HASH));
    serve(repo, request, &[Slot::Logo], default).await
}

/// The info board's break image: the competition's own, else its logo, else
/// the one built into the binary (#291).
pub async fn break_image(Extension(repo): Extension<DbHandle>, request: HeaderMap) -> Response {
    let default = Some((images::DEFAULT_BREAK_SVG, DEFAULT_BREAK_HASH));
    serve(repo, request, &[Slot::Break, Slot::Logo], default).await
}

/// A sponsor's logo; 404 if it has none (a sponsor has no default).
pub async fn sponsor_image(
    Extension(repo): Extension<DbHandle>,
    Path(id): Path<i64>,
    request: HeaderMap,
) -> Response {
    serve(repo, request, &[Slot::Sponsor(id)], None).await
}

/// Serves the first image of `slots` that has bytes, else `default` (its
/// content and hash for the ETag), else 404. The ETag names the image that
/// answers, so a fallback that changes under it is noticed.
async fn serve(
    repo: DbHandle,
    request: HeaderMap,
    slots: &[Slot],
    default: Option<(&'static [u8], u64)>,
) -> Response {
    let mut found = None;
    for slot in slots {
        match repo.image_meta(*slot).await {
            Ok(Some(meta)) => {
                found = Some(meta);
                break;
            }
            Ok(None) => {}
            Err(e) => return failed(e),
        }
    }
    let updated_at = found.map(|(_, updated_at)| updated_at);
    let etag = match (found, default) {
        (Some((id, updated_at)), _) => {
            format!("\"img-{id}-{}\"", updated_at.unix_timestamp_nanos())
        }
        (None, Some((_, hash))) => format!("\"default-{hash:016x}\""),
        (None, None) => return StatusCode::NOT_FOUND.into_response(),
    };
    let last_modified = updated_at
        .and_then(|updated_at| updated_at.to_offset(UtcOffset::UTC).format(HTTP_DATE).ok());

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(POLICY));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(value) = HeaderValue::from_str(&etag) {
        headers.insert(ETAG, value);
    }
    if let Some(value) = last_modified
        .as_deref()
        .and_then(|date| HeaderValue::from_str(date).ok())
    {
        headers.insert(LAST_MODIFIED, value);
    }
    if not_modified(&request, &etag, updated_at) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }

    let stored = match found {
        None => None,
        // `None` if it was removed in between.
        Some((id, _)) => match repo.load_image(id).await {
            Ok(stored) => stored,
            Err(e) => return failed(e),
        },
    };
    let (mime_type, data) = match (stored, default) {
        (Some(stored), _) => (stored.mime_type, stored.data),
        (None, Some((data, _))) => (Format::Svg.mime_type().to_string(), data.to_vec()),
        (None, None) => return StatusCode::NOT_FOUND.into_response(),
    };
    if let Ok(value) = HeaderValue::from_str(&mime_type) {
        headers.insert(CONTENT_TYPE, value);
    }
    (headers, data).into_response()
}

/// Whether the browser's copy is current: by the ETag if it sends one,
/// otherwise by the date (to the second, as HTTP dates go).
fn not_modified(request: &HeaderMap, etag: &str, updated_at: Option<OffsetDateTime>) -> bool {
    if let Some(matches) = if_none_match(request, etag) {
        return matches;
    }
    let since = request
        .get(IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|date| PrimitiveDateTime::parse(date, HTTP_DATE).ok())
        .map(PrimitiveDateTime::assume_utc);
    matches!((updated_at, since), (Some(updated_at), Some(since))
        if updated_at.unix_timestamp() <= since.unix_timestamp())
}

fn failed(e: DbError) -> Response {
    warn!("Couldn't load a competition image: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
