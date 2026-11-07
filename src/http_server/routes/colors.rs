//! The competition's colors at `/api/competition/colors.css`, linked from
//! every page after style.css (see `crate::colors`). The URL never changes,
//! so browsers ask again each time (`no-cache`) and mostly get a 304 by the
//! ETag, a hash of the stylesheet.

use axum::{
    Extension,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, ETAG},
    },
    response::{IntoResponse, Response},
};
use tracing::warn;

use super::etag::{fnv1a, if_none_match};
use crate::{colors, competition::Competition, reloadable_sqlite::ReloadableSqlite};

/// The stylesheet with the competition's colors; without a competition or
/// its own colors, it leaves the defaults.
pub async fn competition_colors(
    Extension(db): Extension<ReloadableSqlite>,
    request: HeaderMap,
) -> Response {
    let db = db.get().await.clone();
    let competition = match Competition::current(&db).await {
        Ok(competition) => competition,
        Err(e) => {
            warn!("Couldn't load the competition's colors: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let css = competition.map_or_else(
        || colors::stylesheet(None, None),
        |competition| {
            colors::stylesheet(
                competition.primary_color.as_ref(),
                competition.secondary_color.as_ref(),
            )
        },
    );
    let etag = format!("\"colors-{:016x}\"", fnv1a(css.as_bytes()));

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&etag) {
        headers.insert(ETAG, value);
    }
    if if_none_match(&request, &etag) == Some(true) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }
    (headers, css).into_response()
}
