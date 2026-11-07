//! The competition's logo (#286, ADR 0009): shown on every page and as the
//! favicon, all from one URL. Without one, the logo built into the binary
//! applies.

use std::{fmt, io::Cursor, str::FromStr};

use image::{
    DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits,
    codecs::{
        jpeg::JpegEncoder,
        png::{CompressionType, FilterType as PngFilter, PngEncoder},
        webp::WebPEncoder,
    },
    imageops::FilterType,
    metadata::Orientation,
};
use time::OffsetDateTime;

/// The logo built into the binary, for a competition without its own and
/// for setup mode.
pub const DEFAULT_SVG: &[u8] =
    include_bytes!("../frontend/src/public/assets/images/nrw-freestyle-cup.svg");

pub const SVG: &str = "image/svg+xml";

/// The largest logo, uploaded or in the competition file.
pub const MAX_SIZE: usize = 1024 * 1024;
/// The longest edge of a stored raster logo, in pixels; larger ones are
/// scaled down.
pub const MAX_EDGE: u32 = 512;
/// The longest edge of a raster logo that's decoded at all, against images
/// that are small as a file but huge in memory.
pub const MAX_DECODED_EDGE: u32 = 4096;

/// Why a logo is refused. The messages are for the admin UI.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LogoError {
    #[error("Das Logo ist größer als 1 MiB.")]
    TooLarge,
    #[error("Das Logo muss ein Bild im Format PNG, JPEG, WebP oder SVG sein.")]
    Unsupported,
    #[error("Das Bild lässt sich nicht lesen: {0}")]
    InvalidImage(String),
    #[error("Das Bild ist größer als {MAX_DECODED_EDGE} × {MAX_DECODED_EDGE} Pixel.")]
    TooManyPixels,
    #[error("Das SVG lässt sich nicht lesen: {0}")]
    InvalidSvg(String),
    #[error(
        "Die Datei ist kein SVG: Ihr äußerstes Element muss <svg \
         xmlns=\"http://www.w3.org/2000/svg\"> sein."
    )]
    NotAnSvg,
    #[error(
        "Das SVG enthält {0}. Ein Logo darf keine Skripte, Ereignis-Attribute (on…), \
         <foreignObject>, Entity-Deklarationen oder Verweise auf andere Dateien enthalten."
    )]
    UnsafeSvg(String),
}

/// The type of an image, told from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Png,
    Jpeg,
    Webp,
    Svg,
}

impl Kind {
    fn of(bytes: &[u8]) -> Option<Kind> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Kind::Png)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Kind::Jpeg)
        } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            Some(Kind::Webp)
        } else if std::str::from_utf8(bytes).is_ok_and(|text| {
            text.trim_start_matches('\u{feff}')
                .trim_start()
                .starts_with('<')
        }) {
            Some(Kind::Svg)
        } else {
            None
        }
    }

    fn mime_type(self) -> &'static str {
        match self {
            Kind::Png => "image/png",
            Kind::Jpeg => "image/jpeg",
            Kind::Webp => "image/webp",
            Kind::Svg => SVG,
        }
    }
}

/// A logo ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct Logo {
    pub mime_type: &'static str,
    pub data: Vec<u8>,
}

impl Logo {
    /// An uploaded logo, its type told from its content.
    pub fn from_upload(bytes: &[u8]) -> Result<Logo, LogoError> {
        if bytes.len() > MAX_SIZE {
            return Err(LogoError::TooLarge);
        }
        let kind = Kind::of(bytes).ok_or(LogoError::Unsupported)?;
        let data = match kind {
            Kind::Svg => {
                // `Kind::of` checked that it's text.
                let text = std::str::from_utf8(bytes).map_err(|_| LogoError::Unsupported)?;
                return Ok(Logo::from(&text.parse::<SvgLogo>()?));
            }
            Kind::Png => scale(bytes, ImageFormat::Png)?,
            Kind::Jpeg => scale(bytes, ImageFormat::Jpeg)?,
            Kind::Webp => scale(bytes, ImageFormat::WebP)?,
        };
        Ok(Logo {
            mime_type: kind.mime_type(),
            data,
        })
    }
}

/// The name of a logo's format for people, e.g. "PNG" for `image/png`.
pub fn format_name(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/webp" => "WebP",
        SVG => "SVG",
        _ => "raster",
    }
}

/// An SVG logo, e.g. `logo_svg` in the competition file: checked like an
/// upload, and stored as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgLogo(String);

impl FromStr for SvgLogo {
    type Err = LogoError;

    fn from_str(text: &str) -> Result<Self, LogoError> {
        if text.len() > MAX_SIZE {
            return Err(LogoError::TooLarge);
        }
        if Kind::of(text.as_bytes()) != Some(Kind::Svg) {
            return Err(LogoError::NotAnSvg);
        }
        check_svg(text)?;
        Ok(SvgLogo(text.to_string()))
    }
}

impl fmt::Display for SvgLogo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&SvgLogo> for Logo {
    fn from(svg: &SvgLogo) -> Self {
        Logo {
            mime_type: SVG,
            data: svg.0.as_bytes().to_vec(),
        }
    }
}

/// A raster logo scaled down to [`MAX_EDGE`] and encoded again in its own
/// format, which drops its metadata (e.g. the camera or GPS position in
/// EXIF). The EXIF orientation is applied first, so the logo stays upright.
fn scale(bytes: &[u8], format: ImageFormat) -> Result<Vec<u8>, LogoError> {
    let error = |e: ImageError| match e {
        ImageError::Limits(_) => LogoError::TooManyPixels,
        e => LogoError::InvalidImage(e.to_string()),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_EDGE);
    limits.max_image_height = Some(MAX_DECODED_EDGE);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(error)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(error)?;
    image.apply_orientation(orientation);
    if image.width() > MAX_EDGE || image.height() > MAX_EDGE {
        image = image.resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3);
    }

    let mut data = Vec::new();
    match format {
        ImageFormat::Jpeg => DynamicImage::ImageRgb8(image.to_rgb8())
            .write_with_encoder(JpegEncoder::new_with_quality(&mut data, 90)),
        ImageFormat::WebP => {
            // The WebP encoder takes 8 bits per channel only.
            let image = if image.color().has_alpha() {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            image.write_with_encoder(WebPEncoder::new_lossless(&mut data))
        }
        _ => image.write_with_encoder(PngEncoder::new_with_quality(
            &mut data,
            CompressionType::Best,
            PngFilter::Adaptive,
        )),
    }
    .map_err(error)?;
    Ok(data)
}

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Checks that an SVG logo can't run scripts or load anything when it's
/// opened on its own, so it's stored as it is (ADR 0009). Served, it gets a
/// restrictive Content-Security-Policy as well; in `<img>` and as favicon,
/// browsers run no scripts anyway.
fn check_svg(text: &str) -> Result<(), LogoError> {
    let unsafe_svg = |what: &str| Err(LogoError::UnsafeSvg(what.to_string()));
    // roxmltree expands entities; there's no reason for a logo to declare
    // any.
    if text.to_ascii_lowercase().contains("<!entity") {
        return unsafe_svg("eine Entity-Deklaration");
    }
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            // Drawing programs write a DOCTYPE, like the built-in logo.
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| LogoError::InvalidSvg(e.to_string()))?;
    let root = document.root_element().tag_name();
    if root.name() != "svg" || root.namespace() != Some(SVG_NAMESPACE) {
        return Err(LogoError::NotAnSvg);
    }
    for node in document.descendants() {
        if let Some(pi) = node.pi()
            && pi.target.eq_ignore_ascii_case("xml-stylesheet")
        {
            return unsafe_svg("ein Stylesheet aus einer anderen Datei");
        }
        if !node.is_element() {
            continue;
        }
        let name = node.tag_name().name().to_ascii_lowercase();
        if name == "script" || name == "foreignobject" {
            return unsafe_svg(&format!("<{}>", node.tag_name().name()));
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            if name.starts_with("on") {
                return unsafe_svg(&format!("das Attribut {}", attribute.name()));
            }
            // Browsers ignore whitespace and control characters in a URL's
            // scheme, e.g. "java\tscript:".
            let value: String = attribute
                .value()
                .chars()
                .filter(|c| !c.is_whitespace() && !c.is_control())
                .collect::<String>()
                .to_ascii_lowercase();
            if value.contains("javascript:") {
                return unsafe_svg("einen javascript:-Verweis");
            }
            if name == "href" && !is_internal_reference(&value) {
                return unsafe_svg(&format!(
                    "einen Verweis auf eine andere Datei ({})",
                    attribute.value().trim()
                ));
            }
        }
    }
    Ok(())
}

/// Whether an `href` stays within the SVG: a fragment or an embedded image.
fn is_internal_reference(href: &str) -> bool {
    href.starts_with('#')
        || ["png", "jpeg", "gif", "webp"]
            .iter()
            .any(|format| href.starts_with(&format!("data:image/{format}")))
}

/// The logo as the database has it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredLogo {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub updated_at: OffsetDateTime,
}

/// The current competition's logo; `None` without one, so the default
/// applies.
pub async fn load(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Option<StoredLogo>> {
    sqlx::query_as!(
        StoredLogo,
        r#"
        SELECT mime_type, data, updated_at as "updated_at: OffsetDateTime"
        FROM competition_logo
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
        "#
    )
    .fetch_optional(db)
    .await
}

/// When the current competition's logo last changed; `None` without one.
/// Reads no image, for the ETag.
pub async fn updated_at(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Option<OffsetDateTime>> {
    sqlx::query_scalar!(
        r#"
        SELECT updated_at as "updated_at: OffsetDateTime"
        FROM competition_logo
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
        "#
    )
    .fetch_optional(db)
    .await
}

/// Sets the logo of the competition `competition_id`.
pub async fn store(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    logo: &Logo,
) -> sqlx::Result<()> {
    let now = OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO competition_logo (competition_id, mime_type, data, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (competition_id) DO UPDATE SET
            mime_type = excluded.mime_type,
            data = excluded.data,
            updated_at = excluded.updated_at
        "#,
        competition_id,
        logo.mime_type,
        logo.data,
        now,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Removes the logo, so the default applies again.
pub async fn remove(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM competition_logo")
        .execute(db)
        .await?;
    Ok(())
}
