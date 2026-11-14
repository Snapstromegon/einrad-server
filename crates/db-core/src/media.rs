//! The competition's images: the logo, the break image and the sponsor
//! logos, all rows of one table so competition queries never read image
//! bytes (#286, #291, ADR 0009, ADR 0010), and the [`MediaRepo`] trait.

use time::OffsetDateTime;

use crate::DbError;

/// The format of a logo, told from its content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    Svg,
}

impl Format {
    /// The format of `bytes` by their first bytes; anything text that starts
    /// like XML might be an SVG.
    pub fn of(bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Format::Png)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Format::Jpeg)
        } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            Some(Format::Webp)
        } else if std::str::from_utf8(bytes).is_ok_and(|text| {
            text.trim_start_matches('\u{feff}')
                .trim_start()
                .starts_with('<')
        }) {
            Some(Format::Svg)
        } else {
            None
        }
    }

    pub fn from_mime_type(mime_type: &str) -> Option<Format> {
        [Format::Png, Format::Jpeg, Format::Webp, Format::Svg]
            .into_iter()
            .find(|format| format.mime_type() == mime_type)
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Webp => "image/webp",
            Format::Svg => "image/svg+xml",
        }
    }

    /// The name for people, e.g. "PNG".
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Webp => "WebP",
            Format::Svg => "SVG",
        }
    }
}

/// A logo ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct Logo {
    pub format: Format,
    pub data: Vec<u8>,
}

/// The logo as the database has it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredLogo {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub updated_at: OffsetDateTime,
}

impl StoredLogo {
    pub fn format(&self) -> Option<Format> {
        Format::from_mime_type(&self.mime_type)
    }

    /// Whether it's `logo`, so storing that changes nothing.
    pub fn is(&self, logo: &Logo) -> bool {
        self.format() == Some(logo.format) && self.data == logo.data
    }
}

/// Which image of the competition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Logo,
    /// The info board's image while no act runs.
    Break,
    /// A sponsor's logo, by the sponsor's id.
    Sponsor(i64),
}

impl Slot {
    /// Whether `image` is the one in this slot.
    pub fn is(self, image: &Image) -> bool {
        match self {
            Slot::Sponsor(id) => image.kind == "sponsor" && image.id == id,
            slot => image.kind == slot.kind(),
        }
    }

    /// The `kind` of the slot's image row.
    pub fn kind(self) -> &'static str {
        match self {
            Slot::Logo => "logo",
            Slot::Break => "break",
            Slot::Sponsor(_) => "sponsor",
        }
    }
}

/// An image row without its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub id: i64,
    pub kind: String,
    pub position: i64,
    pub name: String,
    pub link: Option<String>,
    pub source_url: Option<String>,
    pub mime_type: Option<String>,
    pub has_data: bool,
    pub updated_at: OffsetDateTime,
    pub fetched_at: Option<OffsetDateTime>,
    pub fetch_error: Option<String>,
}

impl Image {
    pub fn format(&self) -> Option<Format> {
        self.mime_type.as_deref().and_then(Format::from_mime_type)
    }
}

/// The competition's images. Every method acts on the current competition.
#[async_trait::async_trait]
pub trait MediaRepo {
    /// All images: logo, break image, then the sponsors in order.
    async fn list_images(&self) -> Result<Vec<Image>, DbError>;

    /// The id and last change of the image in `slot` if it has bytes, so
    /// the ETag reads no image.
    async fn image_meta(&self, slot: Slot) -> Result<Option<(i64, OffsetDateTime)>, DbError>;

    /// The bytes of the image row `id`; `None` without any.
    async fn load_image(&self, id: i64) -> Result<Option<StoredLogo>, DbError>;

    /// The bytes of the logo or the break image; `None` without any (a URL
    /// image that was never downloaded has none).
    async fn load_slot_image(&self, slot: Slot) -> Result<Option<StoredLogo>, DbError>;

    /// Sets the image in `slot` to `logo`, an inline SVG or an upload: it no
    /// longer comes from a URL. `false` if there is no competition or the
    /// sponsor doesn't exist.
    async fn set_image_data(&self, slot: Slot, logo: &Logo) -> Result<bool, DbError>;

    /// Makes `url` (already checked) the source of the image in `slot`. The
    /// old bytes stay until the first good download. The same URL changes
    /// nothing. `false` if there is no competition or the sponsor doesn't
    /// exist.
    async fn set_image_url(&self, slot: Slot, url: &str) -> Result<bool, DbError>;

    /// Removes the logo or break image (so its fallback applies again), or
    /// the sponsor.
    async fn remove_image(&self, slot: Slot) -> Result<(), DbError>;

    /// Adds a sponsor at the end of the list, without an image yet; returns
    /// its id, `None` without a competition.
    async fn add_sponsor(&self, name: &str, link: Option<&str>) -> Result<Option<i64>, DbError>;

    /// Changes a sponsor's name and link; `false` if it doesn't exist.
    async fn update_sponsor(
        &self,
        id: i64,
        name: &str,
        link: Option<&str>,
    ) -> Result<bool, DbError>;

    /// Puts the sponsors in the order of `ids`, if `ids` names every sponsor
    /// exactly once; `false` (nothing changed) otherwise.
    async fn reorder_sponsors(&self, ids: &[i64]) -> Result<bool, DbError>;

    /// Stores what a download of `url` for image `id` gave. `false` if the
    /// image is gone or has another source by now: the result is dropped. A
    /// good download keeps `updated_at` if the bytes didn't change; a failed
    /// one keeps the last good copy.
    async fn store_fetched_image(
        &self,
        id: i64,
        url: &str,
        result: Result<Logo, String>,
    ) -> Result<bool, DbError>;
}
