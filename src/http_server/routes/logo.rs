//! The competition's logo at `/api/competition/logo`, for every page and the
//! favicon (see `crate::logo`). The URL never changes, so browsers ask again
//! each time (`no-cache`) and mostly get a 304 by the ETag, which only reads
//! the logo's `updated_at`.

use axum::{
    Extension,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE,
            IF_NONE_MATCH, LAST_MODIFIED, X_CONTENT_TYPE_OPTIONS,
        },
    },
    response::{IntoResponse, Response},
};
use time::{
    OffsetDateTime, PrimitiveDateTime, UtcOffset, format_description::BorrowedFormatItem, macros,
};
use tracing::warn;

use crate::{logo, reloadable_sqlite::ReloadableSqlite};

/// Opened on its own, an SVG logo runs no scripts and loads nothing
/// (ADR 0009).
const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox";

/// The HTTP date format (RFC 9110, IMF-fixdate).
const HTTP_DATE: &[BorrowedFormatItem<'_>] = macros::format_description!(
    "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
);

/// FNV-1a, so the default logo's ETag changes with the logo.
const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        index += 1;
    }
    hash
}

const DEFAULT_HASH: u64 = fnv1a(logo::DEFAULT_SVG);

/// The competition's logo, or the built-in default without one.
pub async fn competition_logo(
    Extension(db): Extension<ReloadableSqlite>,
    request: HeaderMap,
) -> Response {
    let db = db.get().await.clone();
    let updated_at = match logo::updated_at(&db).await {
        Ok(updated_at) => updated_at,
        Err(e) => return failed(e),
    };
    let etag = match updated_at {
        Some(updated_at) => format!("\"logo-{}\"", updated_at.unix_timestamp_nanos()),
        None => format!("\"default-{DEFAULT_HASH:016x}\""),
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

    let (mime_type, data) = match updated_at {
        None => (logo::SVG.to_string(), logo::DEFAULT_SVG.to_vec()),
        Some(_) => match logo::load(&db).await {
            Ok(Some(stored)) => (stored.mime_type, stored.data),
            // Removed in between.
            Ok(None) => (logo::SVG.to_string(), logo::DEFAULT_SVG.to_vec()),
            Err(e) => return failed(e),
        },
    };
    if let Ok(value) = HeaderValue::from_str(&mime_type) {
        headers.insert(CONTENT_TYPE, value);
    }
    (headers, data).into_response()
}

/// Whether the browser's copy is current: by the ETag if it sends one,
/// otherwise by the date (to the second, as HTTP dates go).
fn not_modified(request: &HeaderMap, etag: &str, updated_at: Option<OffsetDateTime>) -> bool {
    if let Some(tags) = request
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    {
        return tags
            .split(',')
            .map(|tag| tag.trim().trim_start_matches("W/"))
            .any(|tag| tag == etag || tag == "*");
    }
    let since = request
        .get(IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|date| PrimitiveDateTime::parse(date, HTTP_DATE).ok())
        .map(PrimitiveDateTime::assume_utc);
    matches!((updated_at, since), (Some(updated_at), Some(since))
        if updated_at.unix_timestamp() <= since.unix_timestamp())
}

fn failed(e: sqlx::Error) -> Response {
    warn!("Couldn't load the competition logo: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
