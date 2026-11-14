//! The competition's primary and secondary color (#287, ADR 0009): stored
//! in the competition's row, set in the admin UI or the competition file,
//! and served to every page as a stylesheet (`/api/competition/colors.css`)
//! that `frontend/src/css/style.css` derives all other colors from. Without
//! them, the default colors built into style.css apply.

pub use db_core::colors::{ColorError, HexColor};

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
