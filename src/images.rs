//! The competition's images (#286, #291, ADR 0009, ADR 0010): the logo, the
//! break image of the info board and the sponsor logos, all rows of one table
//! (`competition_image`) so competition queries never read image bytes.
//!
//! An image comes from an inline SVG, an upload or a URL. A URL image is
//! downloaded by `crate::image_fetch` and the server serves the local copy;
//! this module only stores what was downloaded.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

use crate::logo::{Format, Logo, StoredLogo};

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

    fn kind(self) -> &'static str {
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

/// All images of the current competition: logo, break image, then the
/// sponsors in order.
pub async fn list(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Vec<Image>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!: i64", kind, position, name, link, source_url, mime_type,
            data IS NOT NULL as "has_data!: bool",
            updated_at as "updated_at: OffsetDateTime",
            fetched_at as "fetched_at: OffsetDateTime", fetch_error
        FROM competition_image
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
        ORDER BY CASE kind WHEN 'logo' THEN 0 WHEN 'break' THEN 1 ELSE 2 END, position, id
        "#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Image {
            id: row.id,
            kind: row.kind,
            position: row.position,
            name: row.name,
            link: row.link,
            source_url: row.source_url,
            mime_type: row.mime_type,
            has_data: row.has_data,
            updated_at: row.updated_at,
            fetched_at: row.fetched_at,
            fetch_error: row.fetch_error,
        })
        .collect())
}

/// The id and last change of the image in `slot` if it has bytes, so the
/// ETag reads no image.
pub async fn meta(
    db: impl sqlx::SqliteExecutor<'_>,
    slot: Slot,
) -> sqlx::Result<Option<(i64, OffsetDateTime)>> {
    let (kind, id) = match slot {
        Slot::Sponsor(id) => ("sponsor", id),
        slot => (slot.kind(), -1),
    };
    let row = sqlx::query!(
        r#"
        SELECT id as "id!: i64", updated_at as "updated_at: OffsetDateTime"
        FROM competition_image
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
            AND data IS NOT NULL AND kind = ? AND (kind != 'sponsor' OR id = ?)
        "#,
        kind,
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.id, row.updated_at)))
}

/// The bytes of the image row `id`; `None` without any.
pub async fn load(db: impl sqlx::SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<StoredLogo>> {
    let row = sqlx::query!(
        r#"
        SELECT mime_type, data, updated_at as "updated_at: OffsetDateTime"
        FROM competition_image
        WHERE id = ? AND data IS NOT NULL
        "#,
        id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|row| {
        Some(StoredLogo {
            mime_type: row.mime_type?,
            data: row.data?,
            updated_at: row.updated_at,
        })
    }))
}

/// The bytes of the logo or the break image of the current competition;
/// `None` without any (a URL image that was never downloaded has none).
pub async fn load_single(
    db: &mut SqliteConnection,
    slot: Slot,
) -> sqlx::Result<Option<StoredLogo>> {
    match meta(&mut *db, slot).await? {
        Some((id, _)) => load(&mut *db, id).await,
        None => Ok(None),
    }
}

/// Sets the image in `slot` to `logo`, an inline SVG or an upload: it no
/// longer comes from a URL. `false` if a sponsor doesn't exist.
pub async fn set_data(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    slot: Slot,
    logo: &Logo,
) -> sqlx::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let mime_type = logo.format.mime_type();
    if let Slot::Sponsor(id) = slot {
        let result = sqlx::query!(
            r#"
            UPDATE competition_image SET
                source_url = NULL, mime_type = ?, data = ?, updated_at = ?,
                fetched_at = NULL, fetch_error = NULL
            WHERE id = ? AND competition_id = ? AND kind = 'sponsor'
            "#,
            mime_type,
            logo.data,
            now,
            id,
            competition_id,
        )
        .execute(db)
        .await?;
        return Ok(result.rows_affected() > 0);
    }
    let kind = slot.kind();
    sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, mime_type, data, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT (competition_id, kind) WHERE kind != 'sponsor' DO UPDATE SET
            source_url = NULL,
            mime_type = excluded.mime_type,
            data = excluded.data,
            updated_at = excluded.updated_at,
            fetched_at = NULL,
            fetch_error = NULL
        "#,
        competition_id,
        kind,
        mime_type,
        logo.data,
        now,
    )
    .execute(db)
    .await?;
    Ok(true)
}

/// Makes `url` the source of the image in `slot`. The old bytes stay (from
/// an SVG, an upload or another URL) until the first good download of `url`
/// replaces them, so a typo in the URL costs nothing; its warning shows
/// meanwhile. The same URL changes nothing. Nothing is downloaded here. `false` if
/// a sponsor doesn't exist.
pub async fn set_url(
    conn: &mut SqliteConnection,
    competition_id: &str,
    slot: Slot,
    url: &ImageUrl,
) -> sqlx::Result<bool> {
    let url = url.to_string();
    let now = OffsetDateTime::now_utc();
    if let Slot::Sponsor(id) = slot {
        let result = sqlx::query!(
            r#"
            UPDATE competition_image SET
                source_url = ?, fetched_at = NULL, fetch_error = NULL
            WHERE id = ? AND competition_id = ? AND kind = 'sponsor'
                AND source_url IS NOT ?
            "#,
            url,
            id,
            competition_id,
            url,
        )
        .execute(&mut *conn)
        .await?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }
        let exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM competition_image
            WHERE id = ? AND competition_id = ? AND kind = 'sponsor'"#,
            id,
            competition_id
        )
        .fetch_one(&mut *conn)
        .await?;
        return Ok(exists > 0);
    }
    let kind = slot.kind();
    sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, source_url, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (competition_id, kind) WHERE kind != 'sponsor' DO UPDATE SET
            source_url = excluded.source_url,
            fetched_at = NULL,
            fetch_error = NULL
        WHERE competition_image.source_url IS NOT excluded.source_url
        "#,
        competition_id,
        kind,
        url,
        now,
    )
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

/// Removes the logo or break image (so its fallback applies again), or the
/// sponsor.
pub async fn remove(db: impl sqlx::SqliteExecutor<'_>, slot: Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Sponsor(id) => {
            sqlx::query!(
                "DELETE FROM competition_image WHERE id = ? AND kind = 'sponsor'",
                id
            )
            .execute(db)
            .await?;
        }
        slot => {
            let kind = slot.kind();
            sqlx::query!("DELETE FROM competition_image WHERE kind = ?", kind)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

/// Adds a sponsor at the end of the list, without an image yet; returns its
/// id.
pub async fn add_sponsor(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    name: &str,
    link: Option<&str>,
) -> sqlx::Result<i64> {
    let now = OffsetDateTime::now_utc();
    let result = sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, position, name, link, updated_at)
        VALUES (?, 'sponsor',
            (SELECT COALESCE(MAX(position) + 1, 0) FROM competition_image
                WHERE competition_id = ? AND kind = 'sponsor'),
            ?, ?, ?)
        "#,
        competition_id,
        competition_id,
        name,
        link,
        now,
    )
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Changes a sponsor's name and link; `false` if it doesn't exist.
pub async fn update_sponsor(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    name: &str,
    link: Option<&str>,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "UPDATE competition_image SET name = ?, link = ? WHERE id = ? AND kind = 'sponsor'",
        name,
        link,
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Moves a sponsor to `position` in the list (counted from 0).
pub async fn set_position(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    position: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE competition_image SET position = ? WHERE id = ? AND kind = 'sponsor'",
        position,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Stores what a download of `url` for image `id` gave. `false` if the image
/// is gone or has another source by now (the admin changed it while the
/// download ran): the result is dropped.
///
/// A good download keeps `updated_at`, so the ETag stays, if the bytes
/// didn't change. A failed one keeps the last good copy.
pub async fn store_fetched(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    url: &str,
    result: Result<Logo, String>,
) -> sqlx::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let affected = match result {
        Ok(logo) => {
            let mime_type = logo.format.mime_type();
            sqlx::query!(
                r#"
                UPDATE competition_image SET
                    updated_at = CASE WHEN data IS ? AND mime_type IS ? THEN updated_at ELSE ? END,
                    mime_type = ?, data = ?, fetched_at = ?, fetch_error = NULL
                WHERE id = ? AND source_url = ?
                "#,
                logo.data,
                mime_type,
                now,
                mime_type,
                logo.data,
                now,
                id,
                url,
            )
            .execute(db)
            .await?
            .rows_affected()
        }
        Err(error) => sqlx::query!(
            r#"
            UPDATE competition_image SET fetched_at = ?, fetch_error = ?
            WHERE id = ? AND source_url = ?
            "#,
            now,
            error,
            id,
            url,
        )
        .execute(db)
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

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
    /// Whether there are bytes to serve: false for a URL that was never
    /// downloaded successfully.
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
    pub async fn load(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Self> {
        let images = list(db).await?;
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
