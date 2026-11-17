//! Exports the competition as a competition file (ADR 0005), e.g. to save a
//! test instance's settings without club data. The import and
//! `--competition` take the file unchanged, and an export of the imported
//! file gives the same file again.
//!
//! In the file: the competition with its id, the features it sets, the
//! photos with their password, the logo and break image as SVG or URL, the sponsors, the colors it sets, the
//! categories with their ages, the heats, the finals (both with the jury
//! settings that differ from the default) and the timeplan entries. Not in it: a raster image (only SVG fits
//! into the file; the header says so), default colors (an import keeps the
//! colors, like the logo, unless the file sets them), anything clubs
//! entered, bookings, fee waivers, users and roles, the act order, timeplan
//! progress, finals placements and the jury plan.

use db_core::{
    Db,
    seed::{CompetitionDump, DumpImage},
};

use crate::{
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig, HeatConfig,
        SponsorConfig, TimeplanEntryConfig,
    },
    finals,
    images::ImageUrl,
    jury::JuryUnitKind,
    logo::{StoredLogoExt, SvgLogo},
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
///
/// # Errors
/// If the database can't be read.
pub async fn export(repo: &dyn Db) -> Result<Option<Export>, db_core::DbError> {
    Ok(repo.dump_competition().await?.map(from_dump))
}

/// The competition file for the rows of the database.
#[must_use]
pub fn from_dump(dump: CompetitionDump) -> Export {
    let CompetitionDump {
        competition,
        images,
        categories,
        heats,
        finals: dump_finals,
        timeplan,
    } = dump;
    let mut left_out = Vec::new();
    let (mut logo_svg, mut logo_url) = (Branding::Keep, Branding::Keep);
    let (mut break_image_svg, mut break_image_url) = (Branding::Keep, Branding::Keep);
    let mut sponsors = Vec::new();
    for image in &images {
        match image.image.kind.as_str() {
            "logo" => {
                (logo_svg, logo_url) = source_of(image, "the logo", &mut left_out);
            }
            "break" => {
                (break_image_svg, break_image_url) =
                    source_of(image, "the break image", &mut left_out);
            }
            _ => {
                let label = format!("the sponsor {}", image.image.name);
                let (svg, url) = source_of(image, &label, &mut left_out);
                sponsors.push(SponsorConfig {
                    name: image.image.name.clone(),
                    link: image.image.link.clone(),
                    svg: svg.value().cloned(),
                    url: url.value().cloned(),
                });
            }
        }
    }

    let categories: Vec<CategoryConfig> = categories
        .into_iter()
        .map(|row| CategoryConfig {
            name: row.name,
            description: row.description,
            act_type: row.act_type,
            single_male: row.single_male,
            min_age: row.min_age,
            under_age: row.under_age,
            einfahrzeit_minutes: minutes(row.einfahrzeit_seconds),
            act_duration_minutes: minutes(row.act_duration_seconds),
            judge_duration_minutes: minutes(row.judge_duration_seconds),
        })
        .collect();

    let group_categories: Vec<String> = categories
        .iter()
        .filter(|c| c.act_type.is_group())
        .map(|c| c.name.clone())
        .collect();
    let heats: Vec<HeatConfig> = heats
        .into_iter()
        .map(|row| {
            // Only a skip that differs from the default (groups skip, others
            // don't) goes into the file.
            let is_group = row
                .categories
                .first()
                .is_some_and(|first| group_categories.contains(first));
            let jury = row.jury.for_file(JuryUnitKind::Heat);
            HeatConfig {
                name: row.name,
                code: Some(row.code),
                description: row.description,
                categories: row.categories,
                threshold: Some(row.threshold),
                skip: (row.skip != is_group).then_some(row.skip),
                jury_registrable: jury[0].map(|v| v != 0),
                jury_deployments: jury[1],
                jury_needed_technical: jury[2],
                jury_needed_performance: jury[3],
                jury_needed_dismounts: jury[4],
            }
        })
        .collect();

    let final_configs: Vec<FinalConfig> = dump_finals
        .into_iter()
        .map(|row| {
            let jury = row.jury.for_file(JuryUnitKind::Final);
            FinalConfig {
                slots: finals::group_slots(row.slot_heats.iter().map(String::as_str)),
                name: row.name,
                code: Some(row.code),
                description: row.description,
                act_type: row.act_type,
                single_male: row.single_male,
                einfahrzeit_minutes: minutes(row.einfahrzeit_seconds),
                act_duration_minutes: minutes(row.act_duration_seconds),
                judge_duration_minutes: minutes(row.judge_duration_seconds),
                jury_registrable: jury[0].map(|v| v != 0),
                jury_deployments: jury[1],
                jury_needed_technical: jury[2],
                jury_needed_performance: jury[3],
                jury_needed_dismounts: jury[4],
            }
        })
        .collect();

    let timeplan = timeplan
        .into_iter()
        .map(|row| TimeplanEntryConfig {
            // Only a labelled start block needs its kind spelled out.
            kind: (row.kind == EntryKind::Acts && row.label.is_some()).then_some(EntryKind::Acts),
            category: row.category,
            final_name: row.final_code,
            label: row.label,
            duration_minutes: row.duration_seconds.map(minutes),
            einfahrzeit_minutes: row.einfahrzeit_seconds.map(minutes),
            show_photos: row.show_photos,
            internal: row.internal,
            earliest_start: row.earliest_start,
        })
        .collect();

    Export {
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
            heats,
            finals: final_configs,
            timeplan,
        },
        left_out,
    }
}

/// How the file gives an image: its URL, or an inline SVG, or not at all (a
/// raster image is noted in `left_out`).
fn source_of(
    dump: &DumpImage,
    label: &str,
    left_out: &mut Vec<String>,
) -> (Branding<SvgLogo>, Branding<ImageUrl>) {
    let image = &dump.image;
    if let Some(url) = &image.source_url {
        return (
            Branding::Keep,
            url.parse().map_or(Branding::Keep, Branding::Set),
        );
    }
    if !image.has_data {
        return (Branding::Keep, Branding::Keep);
    }
    match dump.stored.as_ref().and_then(|stored| stored.svg()) {
        Some(svg) => (Branding::Set(svg), Branding::Keep),
        None => {
            left_out.push(format!("{label} ({})", dump.format_name()));
            (Branding::Keep, Branding::Keep)
        }
    }
}
