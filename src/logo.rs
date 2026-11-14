//! The checks and conversions every image of the competition goes through
//! (#286, #291, ADR 0009): the logo, the break image and the sponsor logos,
//! whether uploaded, written in the competition file or downloaded from a URL
//! (see `crate::images`). The logo built into the binary applies without one.

use std::{fmt, io::Cursor, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use db_core::media::{Format, Logo, StoredLogo};
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

/// The logo built into the binary, for a competition without its own and
/// for setup mode.
pub const DEFAULT_SVG: &[u8] =
    include_bytes!("../frontend/src/public/assets/images/nrw-freestyle-cup.svg");

/// The longest edge of a stored raster image, in pixels (4K); larger ones
/// are scaled down, smaller ones stay as they are.
pub const MAX_EDGE: u32 = 3840;

/// Why a logo is refused. The messages are for the admin UI.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LogoError {
    #[error("Die Datei muss ein Bild im Format PNG, JPEG, WebP oder SVG sein.")]
    Unsupported,
    #[error("Das Bild lässt sich nicht lesen: {0}")]
    InvalidImage(String),
    #[error("Das SVG lässt sich nicht lesen: {0}")]
    InvalidSvg(String),
    #[error(
        "Die Datei ist kein SVG: Ihr äußerstes Element muss <svg \
         xmlns=\"http://www.w3.org/2000/svg\"> sein."
    )]
    NotAnSvg,
    #[error(
        "Das SVG enthält {0}. Ein Bild darf keine Skripte, Ereignis-Attribute (on…), \
         <foreignObject>, Entity-Deklarationen oder Verweise auf andere Dateien enthalten."
    )]
    UnsafeSvg(String),
}

/// An SVG logo, e.g. `logo_svg` in the competition file: checked like an
/// upload, and stored as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgLogo(String);

impl FromStr for SvgLogo {
    type Err = LogoError;

    fn from_str(text: &str) -> Result<Self, LogoError> {
        if Format::of(text.as_bytes()) != Some(Format::Svg) {
            return Err(LogoError::NotAnSvg);
        }
        check_svg(text)?;
        Ok(SvgLogo(text.to_string()))
    }
}

impl<'de> Deserialize<'de> for SvgLogo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for SvgLogo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
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
            format: Format::Svg,
            data: svg.0.as_bytes().to_vec(),
        }
    }
}

/// A raster logo scaled down to [`MAX_EDGE`] and encoded again in its own
/// format, which drops its metadata (e.g. the camera or GPS position in
/// EXIF). The EXIF orientation is applied first, so the logo stays upright.
fn scale(bytes: &[u8], format: ImageFormat) -> Result<Vec<u8>, LogoError> {
    let error = |e: ImageError| LogoError::InvalidImage(e.to_string());
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    // Only trusted admins set images (ADR 0010): any size is decoded and
    // scaled down.
    reader.limits(Limits::no_limits());
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
        // An animation could set a link the check below never sees.
        if name.starts_with("animate") || name == "set" {
            let animated = node
                .attributes()
                .find(|attribute| attribute.name().eq_ignore_ascii_case("attributeName"))
                .map(|attribute| attribute.value().trim().to_ascii_lowercase());
            if animated.is_some_and(|animated| animated == "href" || animated.ends_with(":href")) {
                return unsafe_svg("eine Animation eines Verweises");
            }
        }
        if name == "style" {
            let css: String = node
                .descendants()
                .filter_map(|child| child.text())
                .collect();
            if css_loads_files(&css) {
                return unsafe_svg("CSS, das andere Dateien lädt");
            }
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
            if name == "style" && css_loads_files(attribute.value()) {
                return unsafe_svg("CSS, das andere Dateien lädt");
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

/// Whether CSS loads another file: an `@import`, or a `url()` that isn't
/// a fragment or an embedded image.
fn css_loads_files(css: &str) -> bool {
    let css = css
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    css.contains("@import")
        || css
            .split("url(")
            .skip(1)
            .any(|url| !is_internal_reference(url.trim_start_matches(['\'', '"'])))
}

/// The SVG of a stored logo, e.g. for the competition file; `None` for a
/// raster logo.
pub trait StoredLogoExt {
    fn svg(&self) -> Option<SvgLogo>;
}

impl StoredLogoExt for StoredLogo {
    fn svg(&self) -> Option<SvgLogo> {
        if self.format() != Some(Format::Svg) {
            return None;
        }
        std::str::from_utf8(&self.data).ok()?.parse().ok()
    }
}

/// An uploaded logo, its format told from its content.
pub fn from_upload(bytes: &[u8]) -> Result<Logo, LogoError> {
    let format = Format::of(bytes).ok_or(LogoError::Unsupported)?;
    let data = match format {
        Format::Svg => {
            // `Format::of` checked that it's text.
            let text = std::str::from_utf8(bytes).map_err(|_| LogoError::Unsupported)?;
            return Ok(Logo::from(&text.parse::<SvgLogo>()?));
        }
        Format::Png => scale(bytes, ImageFormat::Png)?,
        Format::Jpeg => scale(bytes, ImageFormat::Jpeg)?,
        Format::Webp => scale(bytes, ImageFormat::WebP)?,
    };
    Ok(Logo { format, data })
}
