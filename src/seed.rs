//! Applies the competition file: the competition, categories, finals,
//! timeplan; judge registrations follow heats and finals.
//!
//! The database is the source of the competition (ADR 0005), the file only
//! an import: it's applied when the database has no competition yet, or on
//! purpose with `--replace-competition` or the admin UI's import. See
//! [`apply`] and [`preview`].

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use db_core::{
    Db,
    seed::{CompetitionPlan, ImagePlan, PlannedEntry, SeedError, SponsorImage, SponsorPlan},
};
use time::{Date, OffsetDateTime, macros::format_description};
use tracing::warn;

pub use db_core::category_ages::{
    ResolvedCategory, birthday_bounds, coverage_warnings, years_before,
};

use crate::{
    act_type::ActType,
    competition::Competition,
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig, HeatConfig,
        Settings, SponsorConfig, TimeplanEntryConfig,
    },
    finals::{self},
    heats::{self, Discipline},
    images::{self, ImageUrl},
    jury::{JuryUnitKind, UnitSettings},
    logo::{Logo, SvgLogo},
    timeplan::EntryKind,
};

/// The lower birthday bound older competition files and databases stored
/// for categories without a maximum age.
pub const OPEN_FROM_BIRTHDAY: Date = time::macros::date!(1900 - 01 - 01);
/// The upper birthday bound older competition files and databases stored
/// for categories without a minimum age.
pub const OPEN_TO_BIRTHDAY: Date = time::macros::date!(9999 - 12 - 31);

/// The age (AGE-4) that a birthday bound of an older competition file or
/// database stands for: its whole months before `determining_day`, e.g.
/// 2016-06-01 against 2027-06-01 is 11 and 2017-12-01 is 9.5. The open
/// bounds 1900-01-01 and 9999-12-31 give `None`. Any other date fails, as it
/// isn't a whole number of months before the determining day: no rounding.
///
/// ```
/// use einrad_server::seed::age_of_birthday_bound;
/// use time::macros::date;
///
/// let day = date!(2027 - 06 - 01);
/// assert_eq!(age_of_birthday_bound(date!(2016 - 06 - 01), day), Ok(Some(11.0)));
/// assert_eq!(age_of_birthday_bound(date!(9999 - 12 - 31), day), Ok(None));
/// assert!(age_of_birthday_bound(date!(2016 - 05 - 15), day).is_err());
/// ```
pub fn age_of_birthday_bound(bound: Date, determining_day: Date) -> Result<Option<f64>, String> {
    if bound == OPEN_FROM_BIRTHDAY || bound == OPEN_TO_BIRTHDAY {
        return Ok(None);
    }
    let months = (determining_day.year() - bound.year()) * 12
        + (determining_day.month() as i32 - bound.month() as i32);
    if bound.day() != determining_day.day() || months < 0 {
        return Err(format!(
            "the birthday bound {bound} isn't a whole number of months before the \
             determining day {determining_day}; give min_age / under_age instead"
        ));
    }
    Ok(Some(f64::from(months) / 12.0))
}

fn minutes_to_seconds(minutes: f64, field: &str, category: &str) -> Result<i64, String> {
    if minutes < 0.0 || !minutes.is_finite() {
        return Err(format!(
            "Category {category}: {field} must not be negative."
        ));
    }
    Ok((minutes * 60.0).round() as i64)
}

/// Calculates the birthday bounds of all categories against `determining_day`
/// and validates them (see [`birthday_bounds`]).
pub fn resolve_categories(
    categories: &[CategoryConfig],
    determining_day: Date,
) -> Result<Vec<ResolvedCategory>, String> {
    let mut names = HashSet::new();
    let mut resolved = Vec::with_capacity(categories.len());
    for category in categories {
        let name = &category.name;
        if name.trim().is_empty() {
            return Err("A category has an empty name.".to_string());
        }
        if !names.insert(name.clone()) {
            return Err(format!("Category {name} is defined more than once."));
        }
        let (from_birthday, to_birthday) =
            birthday_bounds(name, category.min_age, category.under_age, determining_day)?;
        resolved.push(ResolvedCategory {
            name: name.clone(),
            description: category.description.clone(),
            act_type: category.act_type,
            single_male: category.single_male,
            min_age: category.min_age,
            under_age: category.under_age,
            from_birthday,
            to_birthday,
            einfahrzeit_seconds: minutes_to_seconds(
                category.einfahrzeit_minutes,
                "einfahrzeit_minutes",
                name,
            )?,
            act_duration_seconds: minutes_to_seconds(
                category.act_duration_minutes,
                "act_duration_minutes",
                name,
            )?,
            judge_duration_seconds: minutes_to_seconds(
                category.judge_duration_minutes,
                "judge_duration_minutes",
                name,
            )?,
        });
    }
    Ok(resolved)
}

/// Checks the heats of the config (HEAT-1: one act type and gender, no group
/// categories, a category in at most one heat), naming the offender.
pub fn resolve_heats(
    heats: &[HeatConfig],
    configs: &[CategoryConfig],
) -> Result<Vec<ResolvedHeat>, String> {
    let mut known: Vec<heats::CategoryInfo> = configs
        .iter()
        .map(|category| heats::CategoryInfo {
            name: category.name.trim().to_string(),
            discipline: Discipline::new(category.act_type, category.single_male),
            heat: None,
        })
        .collect();
    let mut names = HashSet::new();
    let mut codes = HashSet::new();
    let mut resolved = Vec::with_capacity(heats.len());
    for config in heats {
        let name = config.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err("A heat has an empty name.".to_string());
        }
        // Files refer to a heat by its code, so that names the offender.
        let code = heats::normalize_code(config.code.as_deref(), &name);
        if !codes.insert(code.clone()) {
            return Err(format!("Heat {code} is defined more than once."));
        }
        if !names.insert(name.clone()) {
            return Err(format!(
                "Heat {code}: the name {name} is used by another heat."
            ));
        }
        let categories = heats::normalize_categories(&config.categories);
        let discipline = heats::check(&code, &categories, &known)
            .map_err(|problem| format!("Heat {code}: {}.", problem.english()))?;
        for category in known.iter_mut().filter(|c| categories.contains(&c.name)) {
            category.heat = Some(code.clone());
        }
        let threshold = match config.threshold {
            Some(threshold) if threshold < 0 => {
                return Err(format!("Heat {code}: the threshold can't be negative."));
            }
            Some(threshold) => threshold,
            None => heats::default_threshold(categories.iter().map(|name| {
                configs
                    .iter()
                    .find(|category| category.name.trim() == name)
                    .and_then(|category| category.under_age)
            })),
        };
        let jury = UnitSettings::resolve(
            JuryUnitKind::Heat,
            config.jury_registrable,
            config.jury_deployments,
            config.jury_needed_technical,
            config.jury_needed_performance,
            config.jury_needed_dismounts,
        )
        .map_err(|e| format!("Heat {code}: {e}."))?;
        resolved.push(ResolvedHeat {
            name,
            code,
            description: config
                .description
                .as_ref()
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty()),
            categories,
            discipline,
            threshold,
            // Groups start in their final, so their Vorlauf is skipped
            // unless the file says otherwise (#345).
            skip: config
                .skip
                .unwrap_or_else(|| discipline.is_some_and(Discipline::is_group)),
            jury,
        });
    }
    Ok(resolved)
}

/// Finds categories that belong to no heat (GRP-1). Only a hint: a category
/// can be created before its heat.
pub fn category_heat_warnings(categories: &[String], heats: &[HeatConfig]) -> Vec<String> {
    categories
        .iter()
        .filter(|name| {
            !heats
                .iter()
                .any(|heat| heat.categories.iter().any(|c| c.trim() == name.as_str()))
        })
        .map(|name| format!("Category {name} belongs to no heat; its acts have no Vorlaufklasse."))
        .collect()
}

/// Checks the finals of the config against its heats and converts their
/// durations. Only singles and pairs have finals; every slot group names a
/// heat of the file of the final's discipline (an empty heat fits any).
pub fn resolve_finals(
    finals: &[FinalConfig],
    heats: &[ResolvedHeat],
) -> Result<Vec<ResolvedFinal>, String> {
    let mut names = HashSet::new();
    let mut codes = HashSet::new();
    // The final each heat fills, to fill at most one.
    let mut final_of: HashMap<String, String> = HashMap::new();
    finals
        .iter()
        .map(|config| {
            let name = config.name.trim();
            if name.is_empty() {
                return Err("A final has an empty name.".to_string());
            }
            // Files refer to a final by its code, so that names the offender.
            let code = heats::normalize_code(config.code.as_deref(), name);
            let name = name.to_string();
            if !codes.insert(code.clone()) {
                return Err(format!("Final {code} is defined more than once."));
            }
            if !names.insert(name.clone()) {
                return Err(format!("Final {code}: the name {name} is used by another final."));
            }
            let slots = finals::normalize_slot_groups(&config.slots).map_err(|_| {
                format!(
                    "Final {code}: slots need at least one slot, each group a heat and a count of at least 1, at most {} in all.",
                    finals::MAX_SLOTS
                )
            })?;
            let discipline = Discipline::new(config.act_type, config.single_male);
            for group in &slots {
                let Some(heat) = heats.iter().find(|heat| heat.code == group.heat) else {
                    return Err(format!(
                        "Final {code}: the slots name the heat {}, which the file doesn't define.",
                        group.heat
                    ));
                };
                if heat.discipline.is_some_and(|d| d != discipline) {
                    return Err(format!(
                        "Final {code}: the heat {} has another act type or gender than the final.",
                        heat.code
                    ));
                }
                let first = final_of
                    .entry(heat.code.clone())
                    .or_insert_with(|| code.clone());
                if *first != code {
                    return Err(format!(
                        "Final {code}: the heat {} already fills slots of final {first}; a heat fills at most one final.",
                        heat.code
                    ));
                }
            }
            Ok(ResolvedFinal {
                name: name.clone(),
                code: code.clone(),
                description: config.description.clone(),
                act_type: config.act_type,
                single_male: config.single_male,
                einfahrzeit_seconds: minutes_to_seconds(
                    config.einfahrzeit_minutes,
                    "einfahrzeit_minutes",
                    &code,
                )?,
                act_duration_seconds: minutes_to_seconds(
                    config.act_duration_minutes,
                    "act_duration_minutes",
                    &code,
                )?,
                judge_duration_seconds: minutes_to_seconds(
                    config.judge_duration_minutes,
                    "judge_duration_minutes",
                    &code,
                )?,
                slots,
                jury: UnitSettings::resolve(
                    JuryUnitKind::Final,
                    config.jury_registrable,
                    config.jury_deployments,
                    config.jury_needed_technical,
                    config.jury_needed_performance,
                    config.jury_needed_dismounts,
                )
                .map_err(|e| format!("Final {code}: {e}."))?,
            })
        })
        .collect()
}

/// The act types (singles split by gender) that need at least one category.
const CATEGORY_KINDS: [(ActType, bool); 5] = [
    (ActType::Single, false),
    (ActType::Single, true),
    (ActType::Pair, false),
    (ActType::GroupSmall, false),
    (ActType::GroupLarge, false),
];

/// Finds act types without any category. Their acts never get a category, so
/// they're never scheduled and never show up in the startlist — e.g. groups,
/// if the config was copied from a year without group categories.
pub fn missing_act_type_warnings(categories: &[ResolvedCategory]) -> Vec<String> {
    CATEGORY_KINDS
        .iter()
        .filter(|(act_type, single_male)| {
            !categories.iter().any(|category| {
                category.act_type == *act_type
                    // Only single categories are split by gender.
                    && (*act_type != ActType::Single || category.single_male == *single_male)
            })
        })
        .map(|(act_type, single_male)| {
            format!(
                "No category for {}{}: acts of this type never get a category.",
                act_type.as_str(),
                if *single_male { ", male" } else { "" }
            )
        })
        .collect()
}

/// Finds categories without a timeplan entry: their acts are never scheduled
/// and missing from the startlist export.
pub fn unscheduled_category_warnings(
    timeplan: &[TimeplanEntryConfig],
    category_names: &[String],
) -> Vec<String> {
    category_names
        .iter()
        .filter(|name| {
            !timeplan
                .iter()
                .any(|entry| entry.category.as_ref() == Some(*name))
        })
        .map(|name| {
            format!("Category {name} has no [[timeplan]] entry; its acts are never scheduled.")
        })
        .collect()
}

/// Finds finals without a timeplan entry: their slots are never planned.
pub fn unscheduled_final_warnings(
    timeplan: &[TimeplanEntryConfig],
    final_names: &[String],
) -> Vec<String> {
    final_names
        .iter()
        .filter(|name| {
            !timeplan
                .iter()
                .any(|entry| entry.final_name.as_ref() == Some(*name))
        })
        .map(|name| format!("Final {name} has no [[timeplan]] entry; it is never planned."))
        .collect()
}

/// The kind of a timeplan entry of the file: as given, else a start block
/// with a category or final, else a custom entry.
fn entry_kind(entry: &TimeplanEntryConfig) -> EntryKind {
    EntryKind::derive(
        entry.kind,
        entry.category.is_some() || entry.final_name.is_some(),
    )
}

/// Checks that every timeplan entry is either a category entry for a known
/// category, a final entry for a known final (each final once) or a custom
/// entry with a label and a duration. A start block given as
/// `kind = "acts"` may also have a label, or only a label.
pub fn validate_timeplan(
    timeplan: &[TimeplanEntryConfig],
    category_names: &[String],
    final_names: &[String],
) -> Result<(), String> {
    let mut planned_finals = HashSet::new();
    for (index, entry) in timeplan.iter().enumerate() {
        let position = index + 1;
        let kind = entry_kind(entry);
        if entry.internal && !kind.can_be_internal() {
            return Err(format!(
                "Timeplan entry {position}: only custom entries can be internal."
            ));
        }
        if kind == EntryKind::Custom && entry.einfahrzeit_minutes.is_some() {
            return Err(format!(
                "Timeplan entry {position}: only start blocks have einfahrzeit_minutes."
            ));
        }
        if entry
            .einfahrzeit_minutes
            .is_some_and(|minutes| minutes < 0.0 || !minutes.is_finite())
        {
            return Err(format!(
                "Timeplan entry {position}: einfahrzeit_minutes must not be negative."
            ));
        }
        // An explicit start block: a label besides or instead of a category
        // or final.
        let labelled_block = entry.kind == Some(EntryKind::Acts);
        if labelled_block {
            if entry.category.is_some() && entry.final_name.is_some() {
                return Err(format!(
                    "Timeplan entry {position}: set either category or final, not both."
                ));
            }
            if entry.duration_minutes.is_some() {
                return Err(format!(
                    "Timeplan entry {position}: start blocks get their duration from their acts, remove duration_minutes."
                ));
            }
            if entry.category.is_none()
                && entry.final_name.is_none()
                && entry.label.as_deref().is_none_or(|l| l.trim().is_empty())
            {
                return Err(format!(
                    "Timeplan entry {position}: a start block needs a category, a final or a label."
                ));
            }
        } else if entry.kind == Some(EntryKind::Custom)
            && (entry.category.is_some() || entry.final_name.is_some())
        {
            return Err(format!(
                "Timeplan entry {position}: custom entries have no category and no final."
            ));
        }
        if let Some(final_name) = &entry.final_name {
            if entry.category.is_some() || (entry.label.is_some() && !labelled_block) {
                return Err(format!(
                    "Timeplan entry {position}: set either category, final or label, not several."
                ));
            }
            if !final_names.contains(final_name) {
                return Err(format!(
                    "Timeplan entry {position}: final {final_name} doesn't exist."
                ));
            }
            if !planned_finals.insert(final_name) {
                return Err(format!(
                    "Timeplan entry {position}: final {final_name} is planned more than once."
                ));
            }
            if entry.duration_minutes.is_some() {
                return Err(format!(
                    "Timeplan entry {position}: final entries get their duration from the final, remove duration_minutes."
                ));
            }
            continue;
        }
        if labelled_block {
            if let Some(category) = &entry.category
                && !category_names.contains(category)
            {
                return Err(format!(
                    "Timeplan entry {position}: category {category} doesn't exist."
                ));
            }
            continue;
        }
        match (&entry.category, &entry.label) {
            (Some(category), None) => {
                if !category_names.contains(category) {
                    return Err(format!(
                        "Timeplan entry {position}: category {category} doesn't exist."
                    ));
                }
                if entry.duration_minutes.is_some() {
                    return Err(format!(
                        "Timeplan entry {position}: category entries get their duration from the category, remove duration_minutes."
                    ));
                }
            }
            (None, Some(label)) => {
                if label.trim().is_empty() {
                    return Err(format!("Timeplan entry {position}: label is empty."));
                }
                match entry.duration_minutes {
                    Some(minutes) if minutes >= 0.0 && minutes.is_finite() => {}
                    Some(_) => {
                        return Err(format!(
                            "Timeplan entry {position}: duration_minutes must not be negative."
                        ));
                    }
                    None => {
                        return Err(format!(
                            "Timeplan entry {position} ({label}): custom entries need duration_minutes."
                        ));
                    }
                }
            }
            (Some(_), Some(_)) => {
                return Err(format!(
                    "Timeplan entry {position}: set either category or label, not both."
                ));
            }
            (None, None) => {
                return Err(format!(
                    "Timeplan entry {position}: set either category, final or label."
                ));
            }
        }
    }
    Ok(())
}

pub use db_core::seed::{Applied, Dropped, Part, ResolvedFinal, ResolvedHeat};

pub use db_core::jury::RemapRegistration;

/// The remap file of dropped judge registrations.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemapFile {
    pub jury_registrations: Vec<RemapRegistration>,
}

/// The outcome of [`apply`]. The warnings are logged as well.
#[derive(Debug)]
pub struct Report {
    pub applied: Applied,
    pub warnings: Vec<String>,
    /// The competition id in the file, if there was one.
    pub file_id: Option<String>,
}

fn seed_error(e: SeedError) -> String {
    match e {
        SeedError::Invalid(message) => message,
        SeedError::Db(e) => format!("Database error while applying the competition file: {e}"),
    }
}

/// Everything the file writes for the competition `id`, checked and
/// resolved. The backend only writes it, in one transaction (ADR 0005), so
/// all checks are here, pure. Its warnings are logged.
///
/// # Errors
/// A message for an invalid file.
pub fn plan(file: &CompetitionFile, id: &str) -> Result<CompetitionPlan, String> {
    let competition = CompetitionConfig {
        id: id.to_string(),
        ..file.competition.clone()
    };
    competition.validate()?;
    for warning in competition.payment_warnings() {
        warn!("{warning}");
    }
    let row = Competition::from(&competition);

    let categories = &file.categories;
    let resolved_categories = if categories.is_empty() {
        vec![]
    } else {
        let resolved = resolve_categories(categories, competition.determining_day)?;
        for warning in coverage_warnings(&resolved)
            .into_iter()
            .chain(missing_act_type_warnings(&resolved))
        {
            warn!("{warning}");
        }
        resolved
    };

    let heats = resolve_heats(&file.heats, categories)?;
    if !categories.is_empty() {
        let names: Vec<String> = categories
            .iter()
            .map(|c| c.name.trim().to_string())
            .collect();
        for warning in category_heat_warnings(&names, &file.heats) {
            warn!("{warning}");
        }
    }

    let finals = if file.finals.is_empty() {
        vec![]
    } else {
        resolve_finals(&file.finals, &heats)?
    };

    let timeplan = &file.timeplan;
    if !timeplan.is_empty() {
        let category_names: Vec<String> =
            resolved_categories.iter().map(|c| c.name.clone()).collect();
        let final_names: Vec<String> = finals.iter().map(|f| f.code.clone()).collect();
        validate_timeplan(timeplan, &category_names, &final_names)?;
        for warning in unscheduled_category_warnings(timeplan, &category_names)
            .into_iter()
            .chain(unscheduled_final_warnings(timeplan, &final_names))
        {
            warn!("{warning}");
        }
    }
    let planned = timeplan
        .iter()
        .map(|entry| PlannedEntry {
            kind: entry_kind(entry),
            earliest_start: entry.earliest_start,
            duration_seconds: entry.duration_minutes.map(|m| (m * 60.0).round() as i64),
            einfahrzeit_seconds: entry.einfahrzeit_minutes.map(|m| (m * 60.0).round() as i64),
            label: entry.label.clone(),
            category: entry.category.clone(),
            final_code: entry.final_name.clone(),
            show_photos: entry.show_photos,
            internal: entry.internal,
        })
        .collect();

    Ok(CompetitionPlan {
        competition: row,
        primary_color: competition.primary_color.clone(),
        secondary_color: competition.secondary_color.clone(),
        logo: image_plan(&competition.logo_svg, &competition.logo_url),
        break_image: image_plan(&competition.break_image_svg, &competition.break_image_url),
        sponsors: competition.sponsors.as_deref().map(sponsor_plans),
        categories: resolved_categories,
        heats,
        finals,
        timeplan: planned,
    })
}

/// The logo or the break image from the file's `_svg` and `_url`: either
/// sets it (an SVG wins if both do; `validate` refuses that), an empty one
/// resets it.
fn image_plan(svg: &Branding<SvgLogo>, url: &Branding<ImageUrl>) -> ImagePlan {
    if let Some(svg) = svg.value() {
        ImagePlan::Svg(Logo::from(svg))
    } else if let Some(url) = url.value() {
        ImagePlan::Url(url.to_string())
    } else if matches!(svg, Branding::Reset) || matches!(url, Branding::Reset) {
        ImagePlan::Reset
    } else {
        ImagePlan::Keep
    }
}

fn sponsor_plans(sponsors: &[SponsorConfig]) -> Vec<SponsorPlan> {
    sponsors
        .iter()
        .map(|sponsor| SponsorPlan {
            // Checked by `CompetitionConfig::validate`.
            name: images::clean_name(&sponsor.name).unwrap_or_else(|_| sponsor.name.clone()),
            link: images::clean_link(sponsor.link.as_deref()).unwrap_or(None),
            image: if let Some(svg) = &sponsor.svg {
                Some(SponsorImage::Svg(Logo::from(svg)))
            } else {
                sponsor
                    .url
                    .as_ref()
                    .map(|url| SponsorImage::Url(url.to_string()))
            },
        })
        .collect()
}

/// Applies the competition file to the database. The database is the
/// source of the competition (ADR 0005): the file creates the competition
/// only if the database has none. Otherwise it's ignored with a warning if
/// it differs, unless `replace` is set. Everything happens in one
/// transaction, so an invalid file changes nothing.
pub async fn apply(
    repo: &dyn Db,
    file: Option<&CompetitionFile>,
    replace: bool,
    data_path: &Path,
) -> Result<Report, String> {
    let Some(file) = file else {
        return Ok(Report {
            applied: Applied::NoFile,
            warnings: Vec::new(),
            file_id: None,
        });
    };
    let outcome = repo
        .apply_competition_file(&file.competition.id, replace, &|id: &str| plan(file, id))
        .await
        .map_err(seed_error)?;
    let mut warnings = outcome.warnings;
    let dropped_registrations = outcome.dropped_registrations;

    // Written after the commit, so it only lists registrations that are
    // really gone.
    let remap_file = if dropped_registrations.is_empty() {
        None
    } else {
        let remap = RemapFile {
            jury_registrations: dropped_registrations,
        };
        match write_remap_file(data_path, &remap) {
            Ok(path) => Some(path),
            Err(e) => {
                warnings.push(format!(
                    "{e}. The dropped judge registrations were:\n{}",
                    toml::to_string(&remap).unwrap_or_default()
                ));
                None
            }
        }
    };
    let applied = match outcome.applied {
        Applied::Replaced(dropped) => {
            let dropped = Dropped {
                remap_file,
                ..dropped
            };
            warnings.extend(dropped_warning(&dropped));
            Applied::Replaced(dropped)
        }
        applied => applied,
    };
    for warning in &warnings {
        warn!("{warning}");
    }
    Ok(Report {
        applied,
        warnings,
        file_id: Some(file.competition.id.clone()),
    })
}

/// What [`apply`] with `replace` would do with a file, for the admin UI to
/// confirm before.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct Preview {
    /// The id of the database's competition, which a replace keeps; `None`
    /// if the file would create the competition.
    pub current_id: Option<String>,
    /// The id in the file.
    pub file_id: String,
    /// The file's competition name.
    pub name: String,
    /// What the file changes; empty if it changes nothing.
    pub changes: Vec<Part>,
    /// What a replace would drop.
    pub dropped: Dropped,
}

/// Applies the file as [`apply`] with `replace` does, but in a transaction
/// that's rolled back: the database stays as it is. Fails like [`apply`]
/// for an invalid file.
pub async fn preview(repo: &dyn Db, file: &CompetitionFile) -> Result<Preview, String> {
    let outcome = repo
        .preview_competition_file(&file.competition.id, &|id: &str| plan(file, id))
        .await
        .map_err(seed_error)?;
    Ok(Preview {
        dropped: outcome.dropped,
        current_id: outcome.current_id,
        file_id: file.competition.id.clone(),
        name: file.competition.name.clone(),
        changes: outcome.changes,
    })
}

/// Creates the competition of `file` (just its `[competition]`, the lists
/// stay empty) in setup mode. Refused with a German message while a
/// competition exists: a database has one (ADR 0001). Nothing is written on
/// an error.
pub async fn create(repo: &dyn Db, file: &CompetitionFile) -> Result<(), String> {
    repo.create_competition(&file.competition.id, &|id: &str| plan(file, id))
        .await
        .map_err(seed_error)
}

/// The competition the server starts with, or `None` for setup mode:
/// without a competition the server starts as long as an admin exists, who
/// imports one in the admin UI. Without either it refuses to start, as
/// nobody could import one.
pub async fn startup_competition(repo: &dyn Db) -> Result<Option<Competition>, String> {
    let db_error =
        |e: db_core::DbError| format!("Database error while applying the competition file: {e}");
    if let Some(competition) = repo.current_competition().await.map_err(db_error)? {
        return Ok(Some(competition));
    }
    if !repo.admin_exists().await.map_err(db_error)? {
        return Err(
            "No competition in the database and no admin to import one. Pass a \
             competition file with --competition or COMPETITION (see competition.example.toml), \
             or an admin with --admin, ADMIN or [admin] to start in setup mode and import the \
             competition in the admin UI."
                .to_string(),
        );
    }
    Ok(None)
}

/// Loads the competition file of `settings`, if any, and applies it as the
/// server does on startup (see [`apply`]).
pub async fn apply_settings(repo: &dyn Db, settings: &Settings) -> Result<Report, String> {
    let file = settings
        .competition_file
        .as_deref()
        .map(CompetitionFile::load)
        .transpose()?;
    apply(
        repo,
        file.as_ref(),
        settings.replace_competition,
        &settings.data_path,
    )
    .await
}

/// Checks a competition file the way the start applies it: it's loaded and
/// applied to a throwaway in-memory database, so a broken file fails early
/// without touching the real database.
///
/// # Errors
/// What applying the file reports for an invalid file.
pub async fn check_file(path: &Path) -> Result<(), String> {
    let file = CompetitionFile::load(path)?;
    let repo = db_sqlite::SqliteRepo::in_memory().await;
    // A scratch dir of its own for anything the apply writes, never the
    // system temp dir itself; removed again.
    let scratch = std::env::temp_dir().join(format!("einrad-check-{}", uuid::Uuid::new_v4()));
    let result = apply(&repo, Some(&file), false, &scratch).await.map(|_| ());
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn dropped_warning(dropped: &Dropped) -> Option<String> {
    let summary = dropped.summary();
    if summary.is_empty() {
        return None;
    }
    let remap = dropped
        .remap_file
        .as_ref()
        .map(|path| {
            format!(
                " They are listed in {}, with an empty jury_unit to fill in.",
                path.display()
            )
        })
        .unwrap_or_default();
    Some(format!(
        "Applying the competition file dropped {}.{remap}",
        summary.join(", ")
    ))
}

/// The remap file's content, with how to fill it in.
pub fn remap_content(remap: &RemapFile) -> Result<String, String> {
    Ok(format!(
        "# Judge registrations whose heat or final the competition file dropped.\n\
         # Fill in jury_unit with the heat's Vorlauf (\"heat: E EW\") or the final\n\
         # (\"final: Finale EW\") of the new competition each judge belongs to.\n\n{}",
        toml::to_string(remap).map_err(|e| e.to_string())?
    ))
}

/// Writes `remap` to `data_path/remap/jury-registrations-<timestamp>.toml`.
fn write_remap_file(data_path: &Path, remap: &RemapFile) -> Result<PathBuf, String> {
    let directory = data_path.join("remap");
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("Couldn't create {}: {e}", directory.display()))?;
    let timestamp = OffsetDateTime::now_utc()
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .expect("valid format");
    let path = directory.join(format!("jury-registrations-{timestamp}.toml"));
    let content = remap_content(remap)?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {

    use super::*;
    use time::macros::date;

    const DAY: Date = date!(2027 - 06 - 01);

    /// The lists of a competition file, without its competition.
    #[derive(serde::Deserialize)]
    struct Lists {
        #[serde(default)]
        categories: Vec<CategoryConfig>,
        #[serde(default)]
        timeplan: Vec<TimeplanEntryConfig>,
    }

    fn lists(toml: &str) -> Lists {
        toml::from_str(toml).unwrap()
    }

    fn categories(toml: &str) -> Vec<CategoryConfig> {
        lists(toml).categories
    }

    fn category(name: &str, min: Option<f64>, under: Option<f64>) -> String {
        let mut s = format!("[[categories]]\nname = \"{name}\"\nact_type = \"single\"\n");
        if let Some(min) = min {
            s += &format!("min_age = {min}\n");
        }
        if let Some(under) = under {
            s += &format!("under_age = {under}\n");
        }
        s
    }

    #[test]
    fn age_bounds_follow_the_determining_day() {
        let config =
            categories(&(category("U15", None, Some(15.0)) + &category("15+", Some(15.0), None)));
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].from_birthday, Some(date!(2012 - 06 - 01)));
        assert_eq!(resolved[0].to_birthday, None);
        assert_eq!(resolved[1].from_birthday, None);
        assert_eq!(resolved[1].to_birthday, Some(date!(2012 - 06 - 01)));
        assert!(coverage_warnings(&resolved).is_empty());
    }

    #[test]
    fn the_ages_are_kept_next_to_the_derived_bounds() {
        let config = categories(&category("U13", Some(11.0), Some(13.0)));
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].min_age, Some(11.0));
        assert_eq!(resolved[0].under_age, Some(13.0));
    }

    #[test]
    fn the_bounds_on_29_february_fall_back_to_the_28th() {
        let leap_day = date!(2028 - 02 - 29);
        let config = categories(&category("U13", Some(1.0), Some(13.0)));
        let resolved = resolve_categories(&config, leap_day).unwrap();
        assert_eq!(resolved[0].from_birthday, Some(date!(2015 - 02 - 28)));
        assert_eq!(resolved[0].to_birthday, Some(date!(2027 - 02 - 28)));
    }

    #[test]
    fn birthday_bounds_on_the_day_of_the_month_become_whole_months() {
        assert_eq!(
            age_of_birthday_bound(date!(2016 - 06 - 01), DAY),
            Ok(Some(11.0))
        );
        assert_eq!(
            age_of_birthday_bound(date!(2017 - 12 - 01), DAY),
            Ok(Some(9.5))
        );
        assert_eq!(
            age_of_birthday_bound(date!(1957 - 06 - 01), DAY),
            Ok(Some(70.0))
        );
        let months = age_of_birthday_bound(date!(2016 - 05 - 01), DAY)
            .unwrap()
            .unwrap();
        assert_eq!(years_before(DAY, months), date!(2016 - 05 - 01));
    }

    #[test]
    fn the_open_birthday_bounds_become_empty_ages() {
        assert_eq!(age_of_birthday_bound(OPEN_FROM_BIRTHDAY, DAY), Ok(None));
        assert_eq!(age_of_birthday_bound(OPEN_TO_BIRTHDAY, DAY), Ok(None));
    }

    #[test]
    fn other_birthday_bounds_fail() {
        for bound in [
            date!(2016 - 05 - 15),
            date!(2016 - 06 - 02),
            date!(2028 - 06 - 01),
        ] {
            let error = age_of_birthday_bound(bound, DAY).unwrap_err();
            assert!(error.contains(&bound.to_string()), "{error}");
        }
    }

    #[test]
    fn half_years_are_supported() {
        let config = categories(
            &(category("U9,5", None, Some(9.5)) + &category("U11", Some(9.5), Some(11.0))),
        );
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].from_birthday, Some(date!(2017 - 12 - 01)));
        assert_eq!(resolved[1].from_birthday, Some(date!(2016 - 06 - 01)));
        assert_eq!(resolved[1].to_birthday, Some(date!(2017 - 12 - 01)));
    }

    #[test]
    fn a_category_has_no_birthday_fields() {
        let error = toml::from_str::<Lists>(
            "[[categories]]\nname = \"X\"\nact_type = \"pair\"\nfrom_birthday = 2000-01-01\n",
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("from_birthday"), "{error}");
    }

    #[test]
    fn invalid_categories_are_rejected() {
        let duplicate = categories(&(category("A", None, None) + &category("A", None, None)));
        assert!(
            resolve_categories(&duplicate, DAY)
                .unwrap_err()
                .contains("more than once")
        );

        let inverted = categories(&category("A", Some(15.0), Some(11.0)));
        assert!(
            resolve_categories(&inverted, DAY)
                .unwrap_err()
                .contains("lower than")
        );
    }

    #[test]
    fn durations_are_converted_to_seconds() {
        let config = categories(
            "[[categories]]\nname = \"A\"\nact_type = \"single\"\nact_duration_minutes = 3\njudge_duration_minutes = 4.5\n",
        );
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].act_duration_seconds, 180);
        assert_eq!(resolved[0].judge_duration_seconds, 270);
        assert_eq!(resolved[0].einfahrzeit_seconds, 0);
    }

    #[test]
    fn gaps_and_overlaps_are_reported() {
        let gap =
            categories(&(category("U11", None, Some(11.0)) + &category("15+", Some(15.0), None)));
        let warnings = coverage_warnings(&resolve_categories(&gap, DAY).unwrap());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("match neither"), "{warnings:?}");

        let overlap =
            categories(&(category("U15", None, Some(15.0)) + &category("11+", Some(11.0), None)));
        let warnings = coverage_warnings(&resolve_categories(&overlap, DAY).unwrap());
        assert!(warnings[0].contains("overlap"), "{warnings:?}");

        let open_end = categories(&category("U15", None, Some(15.0)));
        let warnings = coverage_warnings(&resolve_categories(&open_end, DAY).unwrap());
        assert!(warnings[0].contains("born before"), "{warnings:?}");
    }

    #[test]
    fn act_types_without_categories_are_reported() {
        let singles_only =
            categories(&(category("U15", None, Some(15.0)) + &category("15+", Some(15.0), None)));
        let warnings = missing_act_type_warnings(&resolve_categories(&singles_only, DAY).unwrap());
        assert_eq!(
            warnings,
            [
                "No category for single, male: acts of this type never get a category.",
                "No category for pair: acts of this type never get a category.",
                "No category for group_small: acts of this type never get a category.",
                "No category for group_large: acts of this type never get a category.",
            ]
        );

        // The gender filter only matters for singles.
        let mut all = singles_only;
        all.extend(categories(
            "[[categories]]\nname = \"EM\"\nact_type = \"single\"\nsingle_male = true\n\
             [[categories]]\nname = \"P\"\nact_type = \"pair\"\nsingle_male = true\n\
             [[categories]]\nname = \"KG\"\nact_type = \"group_small\"\n\
             [[categories]]\nname = \"GG\"\nact_type = \"group_large\"\n",
        ));
        let warnings = missing_act_type_warnings(&resolve_categories(&all, DAY).unwrap());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn categories_without_timeplan_entry_are_reported() {
        let names = vec!["NEM".to_string(), "KG".to_string()];
        let timeplan = lists(
            "[[timeplan]]\ncategory = \"NEM\"\n[[timeplan]]\nlabel = \"Pause\"\nduration_minutes = 30",
        )
        .timeplan;
        assert_eq!(
            unscheduled_category_warnings(&timeplan, &names),
            ["Category KG has no [[timeplan]] entry; its acts are never scheduled."]
        );
    }

    #[test]
    fn timeplan_entries_are_validated() {
        let names = vec!["NEM".to_string()];
        let parse = |toml: &str| lists(toml).timeplan;

        let finals = vec!["Finale 1".to_string()];
        let validate = |toml: &str| validate_timeplan(&parse(toml), &names, &finals);

        assert!(validate("[[timeplan]]\ncategory = \"NEM\"").is_ok());
        assert!(validate("[[timeplan]]\nlabel = \"Pause\"\nduration_minutes = 30").is_ok());
        assert!(validate("[[timeplan]]\nfinal = \"Finale 1\"").is_ok());
        let unknown = validate("[[timeplan]]\ncategory = \"XYZ\"");
        assert!(unknown.unwrap_err().contains("doesn't exist"));
        let unknown = validate("[[timeplan]]\nfinal = \"Finale 2\"");
        assert!(unknown.unwrap_err().contains("doesn't exist"));
        let no_duration = validate("[[timeplan]]\nlabel = \"Pause\"");
        assert!(no_duration.unwrap_err().contains("duration_minutes"));
        let both = validate("[[timeplan]]\ncategory = \"NEM\"\nlabel = \"Pause\"");
        assert!(both.unwrap_err().contains("not both"));
        let both = validate("[[timeplan]]\nfinal = \"Finale 1\"\ncategory = \"NEM\"");
        assert!(both.unwrap_err().contains("not several"));
        let twice =
            validate("[[timeplan]]\nfinal = \"Finale 1\"\n[[timeplan]]\nfinal = \"Finale 1\"");
        assert!(twice.unwrap_err().contains("more than once"));
        let internal = "[[timeplan]]\nlabel = \"Jury\"\nduration_minutes = 10\ninternal = true";
        assert!(validate(internal).is_ok());
        let internal_block = validate("[[timeplan]]\ncategory = \"NEM\"\ninternal = true");
        assert!(internal_block.unwrap_err().contains("only custom entries"));
    }

    const COMPETITION: &str = r#"
        [competition]
        id = "cup"
        name = "Cup"
        location = "Halle"
        start_date = 2027-06-05
        end_date = 2027-06-06
        determining_day = 2027-06-01
        registration_start = 2026-11-01T00:00:00+01:00
        registration_end = 2027-04-30T23:59:59+02:00
        music_upload_end = 2027-05-20T23:59:59+02:00
        fee_per_start = 15

        [[categories]]
        name = "U15"
        act_type = "single"
        under_age = 15

        [[categories]]
        name = "15+"
        act_type = "single"
        min_age = 15

        [[timeplan]]
        category = "U15"
        earliest_start = 2027-06-05T10:00:00+02:00

        [[timeplan]]
        label = "Pause"
        duration_minutes = 30

        [[timeplan]]
        category = "15+"
    "#;

    #[test]
    fn categories_without_a_heat_give_a_warning() {
        let heat = |name: &str, categories: &[&str]| HeatConfig {
            name: name.to_string(),
            code: None,
            description: None,
            categories: categories.iter().map(|c| c.to_string()).collect(),
            threshold: None,
            skip: None,
            jury_registrable: None,
            jury_deployments: None,
            jury_needed_technical: None,
            jury_needed_performance: None,
            jury_needed_dismounts: None,
        };
        let warnings = category_heat_warnings(
            &["KG".to_string(), "U15".to_string()],
            &[heat("H", &["U15"])],
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("KG"), "{warnings:?}");
    }

    #[test]
    fn old_finals_with_labels_or_categories_are_rejected() {
        for finals in [
            "[[finals]]\nname = \"E EM\"\nact_type = \"single\"\nslots = [{ label = \"U17–U21\", count = 4 }]\n",
            "[[finals]]\nname = \"E EM\"\nact_type = \"single\"\ncategories = [\"U15\"]\nslots = [{ heat = \"A\", count = 4 }]\n",
        ] {
            let err = CompetitionFile::parse(&format!("{COMPETITION}\n{finals}")).unwrap_err();
            assert!(err.contains("Final E EM"), "{err}");
            assert!(err.contains("[[heats]]"), "{err}");
            assert!(err.contains("heat = "), "{err}");
        }
    }
}
