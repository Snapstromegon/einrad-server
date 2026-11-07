//! ETags for what's served at a fixed URL with `no-cache`, like the logo and
//! the colors: browsers ask each time and mostly get a 304.

use axum::http::{HeaderMap, header::IF_NONE_MATCH};

/// FNV-1a, to tag content by a hash.
pub(super) const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        index += 1;
    }
    hash
}

/// Whether the request's `If-None-Match` names `etag`; `None` without one.
pub(super) fn if_none_match(request: &HeaderMap, etag: &str) -> Option<bool> {
    let tags = request.get(IF_NONE_MATCH)?.to_str().ok()?;
    Some(
        tags.split(',')
            .map(|tag| tag.trim().trim_start_matches("W/"))
            .any(|tag| tag == etag || tag == "*"),
    )
}
