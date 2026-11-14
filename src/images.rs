//! The competition's images (#286, #291, ADR 0009, ADR 0010): the logo, the
//! break image of the info board and the sponsor logos, all rows of one table
//! (`competition_image`) so competition queries never read image bytes.
//!
//! An image comes from an inline SVG, an upload or a URL. A URL image is
//! downloaded by `crate::image_fetch` and the server serves the local copy;
//! this module only stores what was downloaded.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;

use crate::logo::Format;

/// The break image built into the binary: the last of the fallbacks of
/// [`Slot::Break`], after the logo.
pub const DEFAULT_BREAK_SVG: &[u8] =
    include_bytes!("../frontend/src/public/assets/images/logo_with_blobs.svg");

/// The sponsor logos built into the binary, offered as presets (#291).
pub const PRESET_SPONSORS: [PresetSponsor; 2] = [
    PresetSponsor {
        id: "gwn",
        name: "GWN",
        svg: include_str!("../frontend/src/public/assets/images/gwn.svg"),
    },
    PresetSponsor {
        id: "heiner_weiss",
        name: "Heiner Weiss",
        svg: include_str!("../frontend/src/public/assets/images/heiner-weiss.svg"),
    },
];

#[derive(Debug, Clone, Copy)]
pub struct PresetSponsor {
    pub id: &'static str,
    pub name: &'static str,
    pub svg: &'static str,
}

pub fn preset(id: &str) -> Option<&'static PresetSponsor> {
    PRESET_SPONSORS.iter().find(|preset| preset.id == id)
}

/// The longest URL an image is downloaded from.
const MAX_URL_LENGTH: usize = 2000;

/// Why a URL isn't an image source. The messages are for the admin UI.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UrlError {
    #[error("Die Adresse ist keine gültige URL: {0}")]
    Invalid(String),
    #[error("Die Adresse muss mit http:// oder https:// beginnen.")]
    Scheme,
    #[error("Die Adresse darf keinen Benutzernamen oder Passwort enthalten.")]
    UserInfo,
    #[error("Die Adresse ist länger als {MAX_URL_LENGTH} Zeichen.")]
    TooLong,
}

/// An `http(s)` URL to download an image from; checked without any request.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageUrl(String);

impl FromStr for ImageUrl {
    type Err = UrlError;

    fn from_str(text: &str) -> Result<Self, UrlError> {
        let text = text.trim();
        if text.len() > MAX_URL_LENGTH {
            return Err(UrlError::TooLong);
        }
        let url = url::Url::parse(text).map_err(|e| UrlError::Invalid(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(UrlError::Scheme);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(UrlError::UserInfo);
        }
        Ok(ImageUrl(url.to_string()))
    }
}

impl fmt::Display for ImageUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ImageUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for ImageUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

pub use db_core::media::{Image, Slot};

/// The longest sponsor name.
const MAX_NAME_LENGTH: usize = 100;

/// A sponsor's name (the image's alt text and its label in the admin UI),
/// trimmed.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Der Name des Sponsors fehlt.".to_string());
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Err(format!(
            "Der Name des Sponsors ist länger als {MAX_NAME_LENGTH} Zeichen."
        ));
    }
    Ok(name.to_string())
}

/// A sponsor's link: empty is none, anything else an `http(s)` URL.
pub fn clean_link(link: Option<&str>) -> Result<Option<String>, String> {
    match link.map(str::trim).filter(|link| !link.is_empty()) {
        None => Ok(None),
        Some(link) => link
            .parse::<ImageUrl>()
            .map(|url| Some(url.to_string()))
            .map_err(|e| format!("Der Link des Sponsors ist ungültig: {e}")),
    }
}

/// Where an image comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// None set: the default applies (a sponsor without one isn't shown).
    Default,
    /// An inline SVG, e.g. from the competition file.
    Svg,
    /// An uploaded image.
    Upload,
    /// Downloaded from the URL.
    Url,
}

/// An image as the competition management shows it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImageState {
    pub source: ImageSource,
    /// The URL of a URL source.
    pub url: Option<String>,
    /// Whether there are bytes to serve: false for a URL image that never had
    /// any (a new URL keeps the old bytes until its first good download).
    pub has_image: bool,
    /// When the URL was last tried.
    pub fetched_at: Option<OffsetDateTime>,
    /// Why the last download failed; the last good copy is served meanwhile.
    pub fetch_error: Option<String>,
    /// Changes with the image; to get a new one past the browser's cache.
    pub version: i64,
}

impl From<&Image> for ImageState {
    fn from(image: &Image) -> Self {
        let source = if image.source_url.is_some() {
            ImageSource::Url
        } else if !image.has_data {
            ImageSource::Default
        } else if image.format() == Some(Format::Svg) {
            ImageSource::Svg
        } else {
            ImageSource::Upload
        };
        ImageState {
            source,
            url: image.source_url.clone(),
            has_image: image.has_data,
            fetched_at: image.fetched_at,
            fetch_error: image.fetch_error.clone(),
            version: image.updated_at.unix_timestamp_nanos() as i64 / 1_000_000,
        }
    }
}

impl ImageState {
    const DEFAULT: ImageState = ImageState {
        source: ImageSource::Default,
        url: None,
        has_image: false,
        fetched_at: None,
        fetch_error: None,
        version: 0,
    };
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SponsorState {
    pub id: i64,
    pub name: String,
    pub link: Option<String>,
    pub image: ImageState,
}

/// A sponsor logo built into the binary, to add as a sponsor.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PresetState {
    pub id: String,
    pub name: String,
}

/// The images of the current competition for the competition management.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CompetitionImages {
    pub logo: ImageState,
    pub break_image: ImageState,
    /// In the order of the info board's footer.
    pub sponsors: Vec<SponsorState>,
    pub presets: Vec<PresetState>,
}

impl CompetitionImages {
    pub async fn load(repo: &dyn db_core::Db) -> Result<Self, db_core::DbError> {
        let images = repo.list_images().await?;
        let single = |kind: &str| {
            images
                .iter()
                .find(|image| image.kind == kind)
                .map_or(ImageState::DEFAULT, ImageState::from)
        };
        Ok(CompetitionImages {
            logo: single("logo"),
            break_image: single("break"),
            sponsors: images
                .iter()
                .filter(|image| image.kind == "sponsor")
                .map(|image| SponsorState {
                    id: image.id,
                    name: image.name.clone(),
                    link: image.link.clone(),
                    image: ImageState::from(image),
                })
                .collect(),
            presets: PRESET_SPONSORS
                .iter()
                .map(|preset| PresetState {
                    id: preset.id.to_string(),
                    name: preset.name.to_string(),
                })
                .collect(),
        })
    }
}
