//! Exports the competition as a competition file (ADR 0005), e.g. to save a
//! test instance's settings without club data. The import and
//! `--competition` take the file unchanged, and an export of the imported
//! file gives the same file again.
//!
//! In the file: the competition with its id, the features it sets, the
//! photos with their password, the logo and break image as SVG or URL, the sponsors, the colors it sets, the
//! categories with their ages, the finals, the timeplan
//! entries and the jury categories. Not in it: a raster image (only SVG fits
//! into the file; the header says so), default colors (an import keeps the
//! colors, like the logo, unless the file sets them), anything clubs
//! entered, bookings, fee waivers, users and roles, the act order, timeplan
//! progress, finals placements and the jury plan.

use time::{Date, OffsetDateTime};

use crate::{
    act_type::ActType,
    competition::Competition,
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig,
        JuryCategoryConfig, SponsorConfig, TimeplanEntryConfig,
    },
    finals,
    images::{self, Image, ImageUrl},
    logo::{Format, SvgLogo},
    timeplan::EntryKind,
};

/// Put in front of the exported file.
const HEADER: &str = "\
# Competition file exported from the database; the import and --competition
# take it unchanged. It contains the photo password, if there is one: share it
# only in trusted channels.

";

fn minutes(seconds: i64) -> f64 {
    seconds as f64 / 60.0
}

/// The current competition as a competition file.
pub struct Export {
    pub file: CompetitionFile,
    /// The raster images the file leaves out, e.g. "the logo (PNG)": only
    /// an SVG fits into the file.
    pub left_out: Vec<String>,
}

impl Export {
    /// The file's content, with a header on what's in it.
    pub fn content(&self) -> Result<String, String> {
        let note = if self.left_out.is_empty() {
            String::new()
        } else {
            format!(
                "# The file leaves out these raster images: only an SVG fits into it. An\n\
                 # import into this database keeps them; elsewhere, upload them again in\n\
                 # the admin UI.\n{}\n",
                self.left_out
                    .iter()
                    .map(|image| format!("# - {image}\n"))
                    .collect::<String>()
            )
        };
        Ok(format!("{HEADER}{note}{}", self.file.to_toml()?))
    }
}

/// The current competition as a competition file; `None` without one.
pub async fn export(conn: &mut sqlx::SqliteConnection) -> sqlx::Result<Option<Export>> {
    let Some(competition) = Competition::current(&mut *conn).await? else {
        return Ok(None);
    };
    let images = images::list(&mut *conn).await?;
    let mut left_out = Vec::new();
    let (mut logo_svg, mut logo_url) = (Branding::Keep, Branding::Keep);
    let (mut break_image_svg, mut break_image_url) = (Branding::Keep, Branding::Keep);
    let mut sponsors = Vec::new();
    for image in &images {
        match image.kind.as_str() {
            "logo" => {
                (logo_svg, logo_url) = source_of(conn, image, "the logo", &mut left_out).await?;
            }
            "break" => {
                (break_image_svg, break_image_url) =
                    source_of(conn, image, "the break image", &mut left_out).await?;
            }
            _ => {
                let label = format!("the sponsor {}", image.name);
                let (svg, url) = source_of(conn, image, &label, &mut left_out).await?;
                sponsors.push(SponsorConfig {
                    name: image.name.clone(),
                    link: image.link.clone(),
                    svg: svg.value().cloned(),
                    url: url.value().cloned(),
                });
            }
        }
    }

    let categories = sqlx::query!(
        r#"
        SELECT
            name as "name!", description, act_type as "act_type: ActType",
            is_single_male, min_age, under_age, einfahrzeit_seconds,
            act_duration_seconds, judge_duration_seconds
        FROM categories ORDER BY "order", name
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| CategoryConfig {
        name: row.name,
        description: row.description,
        act_type: row.act_type,
        single_male: row.is_single_male,
        min_age: row.min_age,
        under_age: row.under_age,
        einfahrzeit_minutes: minutes(row.einfahrzeit_seconds),
        act_duration_minutes: minutes(row.act_duration_seconds),
        judge_duration_minutes: minutes(row.judge_duration_seconds),
    })
    .collect();

    let rows = sqlx::query!(
        r#"
        SELECT
            name as "name!", description, act_type as "act_type: ActType",
            is_single_male, einfahrzeit_seconds, act_duration_seconds,
            judge_duration_seconds
        FROM finals ORDER BY "order", name
        "#
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut final_configs = Vec::with_capacity(rows.len());
    for row in rows {
        let labels = sqlx::query_scalar!(
            "SELECT group_label FROM final_slots WHERE final_name = ? ORDER BY position",
            row.name
        )
        .fetch_all(&mut *conn)
        .await?;
        let categories = sqlx::query_scalar!(
            "SELECT category FROM final_categories WHERE final_name = ? ORDER BY category",
            row.name
        )
        .fetch_all(&mut *conn)
        .await?;
        final_configs.push(FinalConfig {
            slots: finals::group_slots(labels.iter().map(String::as_str)),
            categories,
            name: row.name,
            description: row.description,
            act_type: row.act_type,
            single_male: row.is_single_male,
            einfahrzeit_minutes: minutes(row.einfahrzeit_seconds),
            act_duration_minutes: minutes(row.act_duration_seconds),
            judge_duration_minutes: minutes(row.judge_duration_seconds),
        });
    }

    let timeplan = sqlx::query!(
        r#"
        SELECT
            kind as "kind: EntryKind", category, final_name, label, duration_seconds,
            einfahrzeit_seconds, show_photos, internal,
            earliest_start_time as "earliest_start_time: OffsetDateTime"
        FROM timeplan ORDER BY position, id
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| TimeplanEntryConfig {
        // Only a labelled start block needs its kind spelled out.
        kind: (row.kind == EntryKind::Acts && row.label.is_some()).then_some(EntryKind::Acts),
        category: row.category,
        final_name: row.final_name,
        label: row.label,
        duration_minutes: row.duration_seconds.map(minutes),
        einfahrzeit_minutes: row.einfahrzeit_seconds.map(minutes),
        show_photos: row.show_photos,
        internal: row.internal,
        earliest_start: row.earliest_start_time,
    })
    .collect();

    let jury_categories = sqlx::query!(
        r#"
        SELECT
            name, deployments, day as "day: Date", needed_technical,
            needed_performance, needed_dismounts
        FROM jury_categories ORDER BY sort_order
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| JuryCategoryConfig {
        name: row.name,
        deployments: row.deployments,
        day: row.day,
        needed_technical: row.needed_technical,
        needed_performance: row.needed_performance,
        needed_dismounts: row.needed_dismounts,
    })
    .collect();

    Ok(Some(Export {
        file: CompetitionFile {
            competition: CompetitionConfig {
                logo_svg,
                logo_url,
                break_image_svg,
                break_image_url,
                sponsors: (!sponsors.is_empty()).then_some(sponsors),
                ..CompetitionConfig::from(&competition)
            },
            categories,
            finals: final_configs,
            timeplan,
            jury_categories,
        },
        left_out,
    }))
}

/// How the file gives an image: its URL, or an inline SVG, or not at all (a
/// raster image is noted in `left_out`).
async fn source_of(
    conn: &mut sqlx::SqliteConnection,
    image: &Image,
    label: &str,
    left_out: &mut Vec<String>,
) -> sqlx::Result<(Branding<SvgLogo>, Branding<ImageUrl>)> {
    if let Some(url) = &image.source_url {
        return Ok((
            Branding::Keep,
            url.parse().map_or(Branding::Keep, Branding::Set),
        ));
    }
    if !image.has_data {
        return Ok((Branding::Keep, Branding::Keep));
    }
    match images::load(&mut *conn, image.id)
        .await?
        .and_then(|stored| stored.svg())
    {
        Some(svg) => Ok((Branding::Set(svg), Branding::Keep)),
        None => {
            let format = image.format().map_or("raster", Format::name);
            left_out.push(format!("{label} ({format})"));
            Ok((Branding::Keep, Branding::Keep))
        }
    }
}
