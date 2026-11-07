//! Exports the competition as a competition file (ADR 0005), e.g. to save a
//! test instance's settings without club data. The import and
//! `--competition` take the file unchanged, and an export of the imported
//! file gives the same file again.
//!
//! In the file: the competition with its id, the features it sets, the
//! photos with their password, an SVG logo, the categories with explicit
//! birthday bounds, the finals, the timeplan entries and the jury
//! categories. Not in it: a raster logo (only SVG fits into the file; the
//! header says so), anything clubs entered, bookings, fee waivers, users and
//! roles, the act order, timeplan progress, finals placements and the jury
//! plan.

use time::{Date, OffsetDateTime};

use crate::{
    act_type::ActType,
    competition::Competition,
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig,
        JuryCategoryConfig, TimeplanEntryConfig,
    },
    finals,
    logo::{self, Format},
    seed::{OPEN_FROM_BIRTHDAY, OPEN_TO_BIRTHDAY},
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

/// A stored birthday bound as a date, `None` for an open one.
fn bound(value: Option<OffsetDateTime>, open: Date) -> Option<Date> {
    value.map(|value| value.date()).filter(|date| *date != open)
}

/// The current competition as a competition file.
pub struct Export {
    pub file: CompetitionFile,
    /// The format of a logo the file leaves out, e.g. "PNG": only an SVG
    /// logo fits into the file.
    pub logo_left_out: Option<&'static str>,
}

impl Export {
    /// The file's content, with a header on what's in it.
    pub fn content(&self) -> Result<String, String> {
        let note = self
            .logo_left_out
            .map(|format| {
                format!(
                    "# The competition's logo is a {format} image, which the file leaves out:\n\
                     # only an SVG logo fits into it. An import into this database keeps the\n\
                     # logo; elsewhere, upload it again in the admin UI.\n\n"
                )
            })
            .unwrap_or_default();
        Ok(format!("{HEADER}{note}{}", self.file.to_toml()?))
    }
}

/// The current competition as a competition file; `None` without one.
pub async fn export(conn: &mut sqlx::SqliteConnection) -> sqlx::Result<Option<Export>> {
    let Some(competition) = Competition::current(&mut *conn).await? else {
        return Ok(None);
    };
    let (logo_svg, logo_left_out) = match logo::load(&mut *conn).await? {
        None => (Branding::Keep, None),
        Some(stored) => match stored.svg() {
            Some(svg) => (Branding::Set(svg), None),
            None => (
                Branding::Keep,
                Some(stored.format().map_or("raster", Format::name)),
            ),
        },
    };

    let categories = sqlx::query!(
        r#"
        SELECT
            name as "name!", description, act_type as "act_type: ActType",
            is_single_male, from_birthday as "from_birthday: OffsetDateTime",
            to_birthday as "to_birthday: OffsetDateTime", einfahrzeit_seconds,
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
        min_age: None,
        under_age: None,
        from_birthday: bound(row.from_birthday, OPEN_FROM_BIRTHDAY),
        to_birthday: bound(row.to_birthday, OPEN_TO_BIRTHDAY),
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
                ..CompetitionConfig::from(&competition)
            },
            categories,
            finals: final_configs,
            timeplan,
            jury_categories,
        },
        logo_left_out,
    }))
}
