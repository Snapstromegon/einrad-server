//! The competition's primary and secondary color (#287, ADR 0009): stored
//! in the competition's row, set in the admin UI or the competition file,
//! and served to every page as a stylesheet (`/api/competition/colors.css`)
//! that `frontend/src/css/style.css` derives all other colors from. Without
//! them, the default colors built into style.css apply.

use std::{fmt, str::FromStr};

use crate::config::Branding;

/// A competition's color as lowercase `#rrggbb`, from `#rgb` or `#rrggbb`
/// in any case. Nothing else gets through, so a color can't carry other CSS
/// into the stylesheet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HexColor(String);

/// Why a color is refused. The message is for the admin UI.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("„{0}“ ist keine Farbe: erwartet #rgb oder #rrggbb, z. B. #002d56.")]
pub struct ColorError(String);

impl FromStr for HexColor {
    type Err = ColorError;

    fn from_str(text: &str) -> Result<Self, ColorError> {
        let digits = text
            .trim()
            .strip_prefix('#')
            .filter(|digits| digits.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| ColorError(text.to_string()))?;
        let digits = match digits.len() {
            6 => digits.to_string(),
            3 => digits.chars().flat_map(|c| [c, c]).collect(),
            _ => return Err(ColorError(text.to_string())),
        };
        Ok(HexColor(format!("#{}", digits.to_ascii_lowercase())))
    }
}

impl TryFrom<String> for HexColor {
    type Error = ColorError;

    fn try_from(text: String) -> Result<Self, ColorError> {
        text.parse()
    }
}

impl From<HexColor> for String {
    fn from(color: HexColor) -> Self {
        color.0
    }
}

impl fmt::Display for HexColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The custom properties style.css takes the base colors from.
const PRIMARY_PROPERTY: &str = "--competition-primary-color";
const SECONDARY_PROPERTY: &str = "--competition-secondary-color";

/// The stylesheet with the competition's colors: a declaration for each
/// color it sets, none for a default one.
pub fn stylesheet(primary: Option<&HexColor>, secondary: Option<&HexColor>) -> String {
    let declarations: String = [(PRIMARY_PROPERTY, primary), (SECONDARY_PROPERTY, secondary)]
        .into_iter()
        .filter_map(|(property, color)| color.map(|color| format!("  {property}: {color};\n")))
        .collect();
    let rule = if declarations.is_empty() {
        String::new()
    } else {
        format!(":root {{\n{declarations}}}\n")
    };
    format!(
        "/* The competition's colors (ADR 0009); frontend/src/css/style.css derives\n   \
         every other color from them. Without them, its defaults apply. */\n{rule}"
    )
}

/// Writes the competition's colors: each is kept, reset to the default or
/// set (ADR 0009).
pub async fn write(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    primary: &Branding<HexColor>,
    secondary: &Branding<HexColor>,
) -> sqlx::Result<()> {
    let value = |color: &Branding<HexColor>| color.value().map(HexColor::to_string);
    let (keep_primary, primary) = (primary.is_keep(), value(primary));
    let (keep_secondary, secondary) = (secondary.is_keep(), value(secondary));
    sqlx::query!(
        r#"
        UPDATE competition SET
            primary_color = IIF($1, primary_color, $2),
            secondary_color = IIF($3, secondary_color, $4)
        WHERE id = $5
        "#,
        keep_primary,
        primary,
        keep_secondary,
        secondary,
        competition_id,
    )
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_lowercase_rrggbb() {
        for (text, color) in [
            ("#002d56", "#002d56"),
            ("#1A6B3C", "#1a6b3c"),
            ("#C05", "#cc0055"),
            (" #abc ", "#aabbcc"),
        ] {
            assert_eq!(text.parse::<HexColor>().unwrap().to_string(), color);
        }
    }

    #[test]
    fn anything_but_a_hex_color_is_refused() {
        for text in [
            "",
            "#",
            "002d56",
            "#12345",
            "#1234",
            "#002d5680",
            "#ggg",
            "red",
            "rgb(0, 45, 86)",
            "#002d56; } body { display: none",
            "#ä12",
        ] {
            let error = text.parse::<HexColor>().unwrap_err();
            assert!(error.to_string().contains("#rrggbb"), "{text}: {error}");
        }
    }
}
