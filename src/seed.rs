//! Applies the competition file: the competition, categories, finals,
//! timeplan; judge registrations follow heats and finals.
//!
//! The database is the source of the competition (ADR 0005), the file only
//! an import: it's applied when the database has no competition yet, or on
//! purpose with `--replace-competition` or the admin UI's import. See
//! [`apply`] and [`preview`].

use crate::ids::{CategoryId, FinalId, HeatId, JudgeId, JuryUnitId};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use time::{Date, OffsetDateTime, macros::format_description};
use tracing::{info, warn};

pub use db_core::category_ages::{
    ResolvedCategory, birthday_bounds, coverage_warnings, years_before,
};

use crate::{
    act_type::ActType,
    colors,
    competition::{Competition, upsert_competition},
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig, HeatConfig,
        Settings, SponsorConfig, TimeplanEntryConfig,
    },
    finals::{self, SlotGroup},
    heats::{self, Discipline},
    images::{self, Image, ImageUrl, Slot},
    jury::{JudgeTask, JuryUnitKind, UnitSettings},
    logo::{Logo, SvgLogo},
    reloadable_sqlite::begin_write,
    roles::Role,
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

/// A final from the config with its durations in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFinal {
    /// The readable name.
    pub name: String,
    /// The short code, which the timeplan refers to.
    pub code: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    pub slots: Vec<SlotGroup>,
    pub jury: UnitSettings,
}

/// A heat from the config, checked against HEAT-1.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHeat {
    /// The readable name.
    pub name: String,
    /// The short code, which slots and categories refer to.
    pub code: String,
    pub description: Option<String>,
    pub categories: Vec<String>,
    /// The discipline its categories share; `None` without categories.
    pub discipline: Option<Discipline>,
    /// The file's, or the default for its categories.
    pub threshold: i64,
    pub skip: bool,
    pub jury: UnitSettings,
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

/// What [`apply`] did with the competition file.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    /// No competition file was given.
    NoFile,
    /// The database had no competition; the file created it.
    Created,
    /// The database already has the file's competition.
    Unchanged,
    /// The database has a competition that differs from the file; the file
    /// is ignored.
    Ignored,
    /// The file replaced the database's competition (`replace`).
    Replaced(Dropped),
}

/// A part of the competition that a file can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Settings,
    Images,
    Colors,
    Categories,
    Heats,
    Finals,
    Timeplan,
}

impl Part {
    /// For warnings, e.g. "the categories".
    fn describe(self) -> &'static str {
        match self {
            Part::Settings => "the competition settings",
            Part::Images => "the logo, break image and sponsors",
            Part::Colors => "the colors",
            Part::Categories => "the categories",
            Part::Heats => "the heats",
            Part::Finals => "the finals",
            Part::Timeplan => "the timeplan",
        }
    }
}

fn describe(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|part| part.describe())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a replace drops along with the old competition's planning.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct Dropped {
    pub timeplan_slots: i64,
    /// Acts started or ended, whose progress is reset.
    pub act_progress: i64,
    /// Acts placed in finals slots.
    pub finals_placements: i64,
    /// The jury plan's assignments.
    pub jury_assignments: i64,
    pub jury_plan_published: bool,
    /// Judge registrations whose jury unit (heat or final) has no namesake in
    /// the file.
    pub judge_registrations: Vec<RemapRegistration>,
    /// Where they were written, to fill in `jury_unit` and apply again.
    #[serde(skip)]
    pub remap_file: Option<PathBuf>,
}

impl Dropped {
    /// The dropped items for a warning, e.g. "12 timeplan slots, the
    /// published jury plan".
    fn summary(&self) -> Vec<String> {
        let counted = |count: i64, one: &str, many: &str| {
            (count > 0).then(|| format!("{count} {}", if count == 1 { one } else { many }))
        };
        [
            counted(self.timeplan_slots, "timeplan slot", "timeplan slots"),
            (self.act_progress > 0).then(|| {
                format!(
                    "the progress of {} act{}",
                    self.act_progress,
                    if self.act_progress == 1 { "" } else { "s" }
                )
            }),
            counted(
                self.finals_placements,
                "finals placement",
                "finals placements",
            ),
            counted(
                self.jury_assignments,
                "jury plan assignment",
                "jury plan assignments",
            ),
            self.jury_plan_published
                .then(|| "the published jury plan".to_string()),
            counted(
                self.judge_registrations.len() as i64,
                "judge registration without a matching heat or final",
                "judge registrations without a matching heat or final",
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

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

/// Applies the competition file to the database. The database is the
/// source of the competition (ADR 0005): the file creates the competition
/// only if the database has none. Otherwise it's ignored with a warning if
/// it differs, unless `replace` is set. Everything happens in one
/// transaction, so an invalid file changes nothing.
pub async fn apply(
    db: &sqlx::SqlitePool,
    file: Option<&CompetitionFile>,
    replace: bool,
    data_path: &Path,
) -> Result<Report, String> {
    let mut warnings = Vec::new();
    let Some(file) = file else {
        return Ok(Report {
            applied: Applied::NoFile,
            warnings,
            file_id: None,
        });
    };
    let mut tx = begin_write(db).await.map_err(db_error)?;
    let mut dropped_registrations = Vec::new();
    let applied = match Competition::current(&mut *tx).await.map_err(db_error)? {
        None => {
            let dropped = write(&mut tx, file, &file.competition.id).await?;
            dropped_registrations = dropped.judge_registrations;
            info!(
                "Created competition {} from the competition file",
                file.competition.id
            );
            Applied::Created
        }
        Some(current) if !replace && current.id != file.competition.id => {
            warnings.push(format!(
                "The competition file has the id {}, the database's competition the id {}; \
                 the file is ignored. A competition's id never changes once created.",
                file.competition.id, current.id
            ));
            Applied::Ignored
        }
        Some(current) => {
            // Applied in a savepoint, so the comparison sees exactly what the
            // file would write.
            let before = Snapshot::take(&mut tx).await.map_err(db_error)?;
            let mut savepoint = sqlx::Connection::begin(&mut *tx).await.map_err(db_error)?;
            let written = write(&mut savepoint, file, &current.id).await;
            let differences = match &written {
                Ok(_) => {
                    before.differences(&Snapshot::take(&mut savepoint).await.map_err(db_error)?)
                }
                Err(_) => vec![],
            };
            match written {
                Err(e) if !replace => {
                    warnings.push(format!("The competition file is invalid and ignored: {e}"));
                    Applied::Ignored
                }
                Err(e) => return Err(e),
                Ok(_) if differences.is_empty() => Applied::Unchanged,
                Ok(_) if !replace => {
                    warnings.push(format!(
                        "The competition file differs from the database in {}; it is ignored, \
                         the database is the source of the competition. To apply it anyway, \
                         start with --replace-competition.",
                        describe(&differences)
                    ));
                    Applied::Ignored
                }
                Ok(dropped) => {
                    savepoint.commit().await.map_err(db_error)?;
                    if current.id != file.competition.id {
                        warnings.push(format!(
                            "The competition file has the id {}, the database's competition the \
                             id {}; the replace keeps {}. A competition's id never changes once \
                             created.",
                            file.competition.id, current.id, current.id
                        ));
                    }
                    info!(
                        "Replaced {} of competition {} with the competition file",
                        describe(&differences),
                        current.id
                    );
                    dropped_registrations = dropped.judge_registrations.clone();
                    Applied::Replaced(dropped)
                }
            }
        }
    };
    tx.commit().await.map_err(db_error)?;

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
    let applied = match applied {
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
pub async fn preview(db: &sqlx::SqlitePool, file: &CompetitionFile) -> Result<Preview, String> {
    let mut tx = begin_write(db).await.map_err(db_error)?;
    let current = Competition::current(&mut *tx).await.map_err(db_error)?;
    let before = Snapshot::take(&mut tx).await.map_err(db_error)?;
    let id = current.as_ref().map_or(&file.competition.id, |c| &c.id);
    let dropped = write(&mut tx, file, id).await?;
    let changes = before.differences(&Snapshot::take(&mut tx).await.map_err(db_error)?);
    tx.rollback().await.map_err(db_error)?;
    Ok(Preview {
        dropped: if current.is_some() && !changes.is_empty() {
            dropped
        } else {
            Dropped::default()
        },
        current_id: current.map(|c| c.id),
        file_id: file.competition.id.clone(),
        name: file.competition.name.clone(),
        changes,
    })
}

/// Creates the competition of `file` (just its `[competition]`, the lists
/// stay empty) in setup mode. Refused with a German message while a
/// competition exists: a database has one (ADR 0001). Nothing is written on
/// an error.
pub async fn create(db: &sqlx::SqlitePool, file: &CompetitionFile) -> Result<(), String> {
    let mut tx = begin_write(db).await.map_err(db_error)?;
    if Competition::current(&mut *tx)
        .await
        .map_err(db_error)?
        .is_some()
    {
        return Err(
            "Der Wettkampf lässt sich nicht anlegen, weil es schon einen Wettkampf gibt."
                .to_string(),
        );
    }
    write(&mut tx, file, &file.competition.id).await?;
    tx.commit().await.map_err(db_error)?;
    info!("Created competition {}", file.competition.id);
    Ok(())
}

/// The competition the server starts with, or `None` for setup mode:
/// without a competition the server starts as long as an admin exists, who
/// imports one in the admin UI. Without either it refuses to start, as
/// nobody could import one.
pub async fn startup_competition(db: &sqlx::SqlitePool) -> Result<Option<Competition>, String> {
    if let Some(competition) = Competition::current(db).await.map_err(db_error)? {
        return Ok(Some(competition));
    }
    let admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = $1"#,
        Role::Admin
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;
    if admins == 0 {
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
pub async fn apply_settings(db: &sqlx::SqlitePool, settings: &Settings) -> Result<Report, String> {
    let file = settings
        .competition_file
        .as_deref()
        .map(CompetitionFile::load)
        .transpose()?;
    apply(
        db,
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
    let db = crate::reloadable_sqlite::in_memory_migrated().await;
    // A scratch dir of its own for anything the apply writes, never the
    // system temp dir itself; removed again.
    let scratch = std::env::temp_dir().join(format!("einrad-check-{}", uuid::Uuid::new_v4()));
    let result = apply(&db, Some(&file), false, &scratch).await.map(|_| ());
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn db_error(e: sqlx::Error) -> String {
    format!("Database error while applying the competition file: {e}")
}

/// The competition's settings and lists as the database has them, without
/// generated ids and progress, to tell whether a file changes anything.
#[derive(Debug, PartialEq)]
struct Snapshot {
    competition: Vec<String>,
    images: Vec<String>,
    colors: Vec<String>,
    categories: Vec<String>,
    heats: Vec<String>,
    finals: Vec<String>,
    timeplan: Vec<String>,
}

impl Snapshot {
    async fn take(conn: &mut sqlx::SqliteConnection) -> sqlx::Result<Self> {
        let mut rows = async |sql: &'static str| {
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *conn)
                .await
        };
        Ok(Self {
            competition: rows(
                r#"SELECT json_array(name, location, start_date, end_date, determining_day,
                    registration_start, registration_end, music_upload_end, fee_per_start,
                    max_fee_per_rider, venue, contact_email, announcement_url, photos_url,
                    payment_deadline, payment_account_holder, payment_iban, payment_bic,
                    payment_reference, jury_min_age, jury_deployments_per_two_acts,
                    jury_missing_deployment_fee, jury_registration_end,
                    feature_jury_registration, photos_source, photos_user, photos_password)
                FROM competition"#,
            )
            .await?,
            // `write` only touches an image if it changes; what a download
            // gave (fetched_at, fetch_error) isn't part of the file.
            images: rows(
                r#"SELECT json_array(kind, position, name, link, source_url, mime_type,
                    updated_at, data IS NOT NULL)
                FROM competition_image ORDER BY kind, position, id"#,
            )
            .await?,
            colors: rows(r#"SELECT json_array(primary_color, secondary_color) FROM competition"#)
                .await?,
            categories: rows(
                r#"SELECT json_array(name, description, min_age, under_age, act_type,
                    is_single_male, "order", einfahrzeit_seconds, act_duration_seconds,
                    judge_duration_seconds)
                FROM categories ORDER BY "order" NULLS FIRST, name"#,
            )
            .await?,
            heats: rows(
                r#"SELECT json_array(name, code, description, threshold, skip,
                    jury_registrable, jury_deployments, jury_needed_technical,
                    jury_needed_performance, jury_needed_dismounts,
                    (SELECT json_group_array(name)
                        FROM (SELECT name FROM categories WHERE heat_id = heats.id ORDER BY name)))
                FROM heats ORDER BY code"#,
            )
            .await?,
            finals: rows(
                r#"SELECT json_array(name, code, description, act_type, is_single_male, "order",
                    einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds,
                    jury_registrable, jury_deployments, jury_needed_technical,
                    jury_needed_performance, jury_needed_dismounts,
                    (SELECT json_group_array(json_array(heat, number))
                        FROM (SELECT heats.code AS heat, number FROM final_slots
                            JOIN heats ON heats.id = final_slots.heat_id
                            WHERE final_slots.final_id = finals.id ORDER BY position)))
                FROM finals ORDER BY "order", code"#,
            )
            .await?,
            timeplan: rows(
                r#"SELECT json_array(earliest_start_time, duration_seconds, label,
                    (SELECT name FROM categories WHERE categories.id = timeplan.category_id),
                    (SELECT code FROM finals WHERE finals.id = timeplan.final_id),
                    kind, show_photos, internal, einfahrzeit_seconds)
                FROM timeplan ORDER BY position, id"#,
            )
            .await?,
        })
    }

    /// The parts that differ from `other`.
    fn differences(&self, other: &Self) -> Vec<Part> {
        [
            (self.competition != other.competition, Part::Settings),
            (self.images != other.images, Part::Images),
            (self.colors != other.colors, Part::Colors),
            (self.categories != other.categories, Part::Categories),
            (self.heats != other.heats, Part::Heats),
            (self.finals != other.finals, Part::Finals),
            (self.timeplan != other.timeplan, Part::Timeplan),
        ]
        .into_iter()
        .filter_map(|(differs, what)| differs.then_some(what))
        .collect()
    }
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

/// Writes the file's competition with `id`, its categories, heats, finals
/// and timeplan, replacing what the database has (ADR 0005): the planning
/// that depends on them is dropped, club data is kept. Judge registrations
/// follow their heat or final by name; the others are returned for the remap
/// file.
async fn write(
    conn: &mut sqlx::SqliteConnection,
    file: &CompetitionFile,
    id: &str,
) -> Result<Dropped, String> {
    let competition = CompetitionConfig {
        id: id.to_string(),
        ..file.competition.clone()
    };
    competition.validate()?;
    for warning in competition.payment_warnings() {
        warn!("{warning}");
    }

    let mut dropped = Dropped {
        timeplan_slots: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM timeplan_slots"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db_error)?,
        act_progress: sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i64" FROM acts
            WHERE started_at IS NOT NULL OR ended_at IS NOT NULL OR music_started_at IS NOT NULL
            "#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db_error)?,
        finals_placements: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM final_slots WHERE act_id IS NOT NULL"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db_error)?,
        jury_assignments: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM jury_assignments"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db_error)?,
        jury_plan_published: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM competition WHERE jury_plan_published_at IS NOT NULL"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db_error)?
            > 0,
        ..Default::default()
    };
    let registrations = sqlx::query!(
        r#"
        SELECT
            judge.id as "judge_id!: JudgeId",
            clubs.name as "club!",
            judge.firstname as "firstname!",
            judge.lastname as "lastname!",
            judge_registration.task as "task!: JudgeTask",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            view_jury_unit.name as "unit_name!"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN clubs ON clubs.id = judge.club_id
        JOIN view_jury_unit ON view_jury_unit.id = judge_registration.unit_id
        ORDER BY clubs.name, judge.lastname, judge.firstname, view_jury_unit.kind DESC,
            view_jury_unit.name, judge_registration.task
        "#
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_error)?;

    for table in [
        "jury_assignments",
        "judge_registration",
        "timeplan_slots",
        "timeplan",
        "final_slots",
        "finals",
        "categories",
        "heats",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
    }

    upsert_competition(&mut *conn, &competition)
        .await
        .map_err(db_error)?;
    colors::write(
        &mut *conn,
        &competition.id,
        &competition.primary_color,
        &competition.secondary_color,
    )
    .await
    .map_err(db_error)?;
    // The app uses one competition. A row left over from an earlier id
    // would win in `Competition::current` if it starts later.
    sqlx::query!("DELETE FROM competition WHERE id != $1", competition.id)
        .execute(&mut *conn)
        .await
        .map_err(db_error)?;
    write_images(conn, &competition).await.map_err(db_error)?;
    sqlx::query!("UPDATE acts SET started_at = NULL, ended_at = NULL, music_started_at = NULL")
        .execute(&mut *conn)
        .await
        .map_err(db_error)?;
    sqlx::query!("UPDATE competition SET jury_plan_published_at = NULL")
        .execute(&mut *conn)
        .await
        .map_err(db_error)?;
    let categories = &file.categories;
    let finals = &file.finals;
    let timeplan = &file.timeplan;

    if !categories.is_empty() {
        let determining_day = Competition::current(&mut *conn)
            .await
            .map_err(db_error)?
            .map(|c| c.determining_day)
            .ok_or("Categories need a [competition] with a determining_day.")?;
        let resolved = resolve_categories(categories, determining_day)?;
        for warning in coverage_warnings(&resolved)
            .into_iter()
            .chain(missing_act_type_warnings(&resolved))
        {
            warn!("{warning}");
        }
        for (index, category) in resolved.iter().enumerate() {
            let order = index as i64 + 1;
            let category_id = CategoryId::new();
            sqlx::query!(
                r#"
                INSERT INTO categories (
                    id, name, description, min_age, under_age, act_type,
                    is_single_male, "order", einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                "#,
                category_id,
                category.name,
                category.description,
                category.min_age,
                category.under_age,
                category.act_type,
                category.single_male,
                order,
                category.einfahrzeit_seconds,
                category.act_duration_seconds,
                category.judge_duration_seconds,
            )
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
        }
        info!(
            "Seeded {} categories from the competition file",
            resolved.len()
        );
    }

    let heats = resolve_heats(&file.heats, categories)?;
    // The file names heats and finals; the database refers to them by id.
    let mut heat_ids: HashMap<String, HeatId> = HashMap::new();
    for heat in &heats {
        if heat.categories.is_empty() {
            warn!(
                "Heat {} has no categories; its finalist slots stay empty until an admin assigns them.",
                heat.name
            );
        }
        let heat_id = HeatId::new();
        heat_ids.insert(heat.code.clone(), heat_id);
        sqlx::query!(
            r#"
            INSERT INTO heats (
                id, name, code, description, threshold, skip, jury_registrable, jury_deployments,
                jury_needed_technical, jury_needed_performance, jury_needed_dismounts
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            heat_id,
            heat.name,
            heat.code,
            heat.description,
            heat.threshold,
            heat.skip,
            heat.jury.registrable,
            heat.jury.deployments,
            heat.jury.needed_technical,
            heat.jury.needed_performance,
            heat.jury.needed_dismounts,
        )
        .execute(&mut *conn)
        .await
        .map_err(db_error)?;
        db_sqlite::structure::set_heat_categories(conn, heat_id, &heat.categories)
            .await
            .map_err(db_error)?;
    }
    if !categories.is_empty() {
        let names: Vec<String> = categories
            .iter()
            .map(|c| c.name.trim().to_string())
            .collect();
        for warning in category_heat_warnings(&names, &file.heats) {
            warn!("{warning}");
        }
    }
    if !heats.is_empty() {
        info!("Seeded {} heats from the competition file", heats.len());
    }

    if !finals.is_empty() {
        let resolved = resolve_finals(finals, &heats)?;
        for (order, item) in (1_i64..).zip(&resolved) {
            let final_id = FinalId::new();
            sqlx::query!(
                r#"
                INSERT INTO finals (
                    id, name, code, description, act_type, is_single_male, "order",
                    einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds,
                    jury_registrable, jury_deployments, jury_needed_technical,
                    jury_needed_performance, jury_needed_dismounts
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                "#,
                final_id,
                item.name,
                item.code,
                item.description,
                item.act_type,
                item.single_male,
                order,
                item.einfahrzeit_seconds,
                item.act_duration_seconds,
                item.judge_duration_seconds,
                item.jury.registrable,
                item.jury.deployments,
                item.jury.needed_technical,
                item.jury.needed_performance,
                item.jury.needed_dismounts,
            )
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
            // `resolve_finals` checked that every slot group's heat is in
            // the file.
            let groups: Vec<finals::SlotGroupInput> = item
                .slots
                .iter()
                .map(|group| finals::SlotGroupInput {
                    heat_id: heat_ids[&group.heat],
                    count: group.count,
                })
                .collect();
            db_sqlite::structure::replace_final_slots(conn, final_id, &groups)
                .await
                .map_err(db_error)?;
        }
        info!("Seeded {} finals from the competition file", resolved.len());
    }

    if !timeplan.is_empty() {
        let category_names: Vec<String> =
            sqlx::query_scalar!(r#"SELECT name as "name!" FROM categories"#)
                .fetch_all(&mut *conn)
                .await
                .map_err(db_error)?;
        let final_names: Vec<String> = sqlx::query_scalar!(r#"SELECT code as "code!" FROM finals"#)
            .fetch_all(&mut *conn)
            .await
            .map_err(db_error)?;
        validate_timeplan(timeplan, &category_names, &final_names)?;
        for warning in unscheduled_category_warnings(timeplan, &category_names)
            .into_iter()
            .chain(unscheduled_final_warnings(timeplan, &final_names))
        {
            warn!("{warning}");
        }
        for (position, entry) in (1_i64..).zip(timeplan) {
            let duration_seconds = entry.duration_minutes.map(|m| (m * 60.0).round() as i64);
            let einfahrzeit_seconds = entry.einfahrzeit_minutes.map(|m| (m * 60.0).round() as i64);
            let kind = entry_kind(entry);
            // `validate_timeplan` checked that the final exists.
            let final_id = match &entry.final_name {
                Some(name) => Some(
                    sqlx::query_scalar!(
                        r#"SELECT id as "id: FinalId" FROM finals WHERE code = $1"#,
                        name
                    )
                    .fetch_one(&mut *conn)
                    .await
                    .map_err(db_error)?,
                ),
                None => None,
            };
            sqlx::query!(
                r#"
                INSERT INTO timeplan (
                    earliest_start_time, duration_seconds, label, category_id, final_id, kind,
                    show_photos, internal, position, einfahrzeit_seconds
                )
                VALUES ($1, $2, $3, (SELECT id FROM categories WHERE name = $4), $5, $6, $7, $8, $9, $10)
                "#,
                entry.earliest_start,
                duration_seconds,
                entry.label,
                entry.category,
                final_id,
                kind,
                entry.show_photos,
                entry.internal,
                position,
                einfahrzeit_seconds,
            )
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
        }
        info!(
            "Seeded {} timeplan entries from the competition file",
            timeplan.len()
        );
    }

    for registration in registrations {
        let unit = sqlx::query_scalar!(
            r#"SELECT id as "id!: JuryUnitId" FROM view_jury_unit WHERE kind = $1 AND name = $2"#,
            registration.unit_kind,
            registration.unit_name
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_error)?;
        match unit {
            Some(unit) => {
                let (heat_id, final_id) = crate::jury::unit_columns(registration.unit_kind, unit);
                sqlx::query!(
                    "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)",
                    registration.judge_id,
                    heat_id,
                    final_id,
                    registration.task,
                )
                .execute(&mut *conn)
                .await
                .map_err(db_error)?;
            }
            None => dropped.judge_registrations.push(RemapRegistration {
                club: registration.club,
                firstname: registration.firstname,
                lastname: registration.lastname,
                task: registration.task,
                old_jury_unit: crate::jury::unit_label(
                    registration.unit_kind,
                    &registration.unit_name,
                ),
                jury_unit: String::new(),
            }),
        }
    }
    Ok(dropped)
}

/// Applies the file's logo, break image and sponsors: they're kept unless
/// the file sets them (ADR 0009, ADR 0010), and only written if they change,
/// so an unchanged file changes nothing and a download that's already there
/// stays.
async fn write_images(
    conn: &mut sqlx::SqliteConnection,
    competition: &CompetitionConfig,
) -> sqlx::Result<()> {
    write_single_image(
        conn,
        &competition.id,
        Slot::Logo,
        &competition.logo_svg,
        &competition.logo_url,
    )
    .await?;
    write_single_image(
        conn,
        &competition.id,
        Slot::Break,
        &competition.break_image_svg,
        &competition.break_image_url,
    )
    .await?;
    if let Some(sponsors) = &competition.sponsors {
        write_sponsors(conn, &competition.id, sponsors).await?;
    }
    Ok(())
}

/// The logo or the break image from the file's `_svg` and `_url`: either
/// sets it (an SVG wins if both do; `validate` refuses that), an empty one
/// resets it.
async fn write_single_image(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    slot: Slot,
    svg: &Branding<SvgLogo>,
    url: &Branding<ImageUrl>,
) -> sqlx::Result<()> {
    if let Some(svg) = svg.value() {
        let current = images::list(&mut *conn)
            .await?
            .into_iter()
            .find(|image| slot.is(image));
        write_svg(conn, competition_id, slot, current.as_ref(), svg).await
    } else if let Some(url) = url.value() {
        images::set_url(conn, competition_id, slot, url)
            .await
            .map(|_| ())
    } else if matches!(svg, Branding::Reset) || matches!(url, Branding::Reset) {
        images::remove(&mut *conn, slot).await
    } else {
        Ok(())
    }
}

/// Sets the image in `slot` to the inline `svg`, unless it already is.
async fn write_svg(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    slot: Slot,
    current: Option<&Image>,
    svg: &SvgLogo,
) -> sqlx::Result<()> {
    let new = Logo::from(svg);
    if let Some(current) = current.filter(|c| c.source_url.is_none() && c.has_data)
        && images::load(&mut *conn, current.id)
            .await?
            .is_some_and(|stored| stored.is(&new))
    {
        return Ok(());
    }
    images::set_data(&mut *conn, competition_id, slot, &new).await?;
    Ok(())
}

/// Makes the sponsors the file's list. A sponsor of the same name is the
/// same sponsor, so it keeps its image if the file gives none, and a list
/// that equals the database's changes nothing.
async fn write_sponsors(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    sponsors: &[SponsorConfig],
) -> sqlx::Result<()> {
    let mut unused: Vec<Image> = images::list(&mut *conn)
        .await?
        .into_iter()
        .filter(|image| image.kind == "sponsor")
        .collect();
    for (position, sponsor) in (0..).zip(sponsors) {
        // Checked by `CompetitionConfig::validate`.
        let name = images::clean_name(&sponsor.name).unwrap_or_else(|_| sponsor.name.clone());
        let link = images::clean_link(sponsor.link.as_deref()).unwrap_or(None);
        let current = unused
            .iter()
            .position(|image| image.name == name)
            .map(|index| unused.remove(index));
        let id = match &current {
            Some(current) => {
                if current.link != link {
                    images::update_sponsor(&mut *conn, current.id, &name, link.as_deref()).await?;
                }
                if current.position != position {
                    images::set_position(&mut *conn, current.id, position).await?;
                }
                current.id
            }
            None => {
                let id =
                    images::add_sponsor(&mut *conn, competition_id, &name, link.as_deref()).await?;
                images::set_position(&mut *conn, id, position).await?;
                id
            }
        };
        if let Some(svg) = &sponsor.svg {
            write_svg(
                conn,
                competition_id,
                Slot::Sponsor(id),
                current.as_ref(),
                svg,
            )
            .await?;
        } else if let Some(url) = &sponsor.url {
            images::set_url(conn, competition_id, Slot::Sponsor(id), url).await?;
        }
    }
    for gone in unused {
        images::remove(&mut *conn, Slot::Sponsor(gone.id)).await?;
    }
    Ok(())
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

    /// Directory removed again when dropped.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("cup-seed-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn apply_file(
        db: &sqlx::SqlitePool,
        toml: &str,
        replace: bool,
        data_path: &Path,
    ) -> Result<Report, String> {
        let file = CompetitionFile::parse(toml).unwrap();
        apply(db, Some(&file), replace, data_path).await
    }

    async fn seed_file(db: &sqlx::SqlitePool, toml: &str) -> Result<(), String> {
        let data = TempDir::new();
        apply_file(db, toml, false, &data.0).await.map(|_| ())
    }

    async fn count(db: &sqlx::SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn migrations_leave_tables_empty() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        for table in [
            "competition",
            "categories",
            "timeplan",
            "users",
            "clubs",
            "rider",
            "acts",
        ] {
            assert_eq!(count(&db, table).await, 0, "{table}");
        }
    }

    #[tokio::test]
    async fn a_changed_file_on_the_next_start_changes_nothing_and_warns() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        let created = apply_file(&db, COMPETITION, false, &data.0).await.unwrap();
        assert_eq!(created.applied, Applied::Created);
        assert!(created.warnings.is_empty(), "{:?}", created.warnings);

        let unchanged = apply_file(&db, COMPETITION, false, &data.0).await.unwrap();
        assert_eq!(unchanged.applied, Applied::Unchanged);
        assert!(unchanged.warnings.is_empty(), "{:?}", unchanged.warnings);

        let changed = COMPETITION.replace("name = \"Cup\"", "name = \"Cup 2\"");
        let report = apply_file(&db, &changed, false, &data.0).await.unwrap();
        assert_eq!(report.applied, Applied::Ignored);
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(
            report.warnings[0].contains("the competition settings"),
            "{:?}",
            report.warnings
        );
        assert!(
            report.warnings[0].contains("--replace-competition"),
            "{:?}",
            report.warnings
        );
        assert_eq!(
            Competition::current(&db).await.unwrap().unwrap().name,
            "Cup"
        );
    }

    #[tokio::test]
    async fn any_changed_detail_counts_as_a_difference() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, &planned_competition(), false, &data.0)
            .await
            .unwrap();
        let cases = [
            ("under_age = 15", "under_age = 14", "the categories"),
            ("count = 2", "count = 3", "the finals"),
            (
                "duration_minutes = 30",
                "duration_minutes = 45",
                "the timeplan",
            ),
            (
                "name = \"Vorlauf\"",
                "name = \"Vorlauf\"\n        jury_deployments = 1",
                "the heats",
            ),
            (
                "name = \"Finale\"\n        act_type",
                "name = \"Finale\"\n        jury_registrable = false\n        act_type",
                "the finals",
            ),
        ];
        for (from, to, expected) in cases {
            let changed = planned_competition().replace(from, to);
            let report = apply_file(&db, &changed, false, &data.0).await.unwrap();
            assert_eq!(report.applied, Applied::Ignored, "{to}");
            assert!(
                report.warnings.len() == 1 && report.warnings[0].contains(expected),
                "{to}: {:?}",
                report.warnings
            );
        }
    }

    #[tokio::test]
    async fn an_invalid_file_on_the_next_start_is_ignored_with_a_warning() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

        let invalid = COMPETITION.replace("category = \"15+\"", "category = \"missing\"");
        let report = apply_file(&db, &invalid, false, &data.0).await.unwrap();
        assert_eq!(report.applied, Applied::Ignored);
        assert!(
            report.warnings.len() == 1 && report.warnings[0].contains("missing"),
            "{:?}",
            report.warnings
        );
        assert_eq!(count(&db, "timeplan").await, 3);
    }

    #[tokio::test]
    async fn replacing_with_an_unchanged_file_keeps_the_planning() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, &planned_competition(), false, &data.0)
            .await
            .unwrap();
        add_club_data_and_planning(&db).await;

        let report = apply_file(&db, &planned_competition(), true, &data.0)
            .await
            .unwrap();
        assert_eq!(report.applied, Applied::Unchanged);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(count(&db, "timeplan_slots").await, 1);
        assert_eq!(count(&db, "jury_assignments").await, 1);
    }

    #[tokio::test]
    async fn a_file_with_another_id_is_ignored_with_a_warning() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

        let renamed = COMPETITION.replace(r#"id = "cup""#, r#"id = "cup-2""#);
        let report = apply_file(&db, &renamed, false, &data.0).await.unwrap();
        assert_eq!(report.applied, Applied::Ignored);
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(
            report.warnings[0].contains("id cup-2")
                && report.warnings[0].contains("cup")
                && report.warnings[0].contains("never changes"),
            "{:?}",
            report.warnings
        );
        assert_eq!(count(&db, "competition").await, 1);
        assert_eq!(Competition::current(&db).await.unwrap().unwrap().id, "cup");
    }

    /// [`COMPETITION`] with a heat and a final, the two jury units.
    fn planned_competition() -> String {
        format!(
            r#"{COMPETITION}
        [[timeplan]]
        final = "Finale"

        [[heats]]
        name = "Vorlauf"
        categories = ["U15"]

        [[finals]]
        name = "Finale"
        act_type = "single"
        slots = [{{ heat = "Vorlauf", count = 2 }}]
        "#
        )
    }

    /// A club with a rider, an act, a judge registered for both jury
    /// units and a booking, plus planning: the act in a timeplan slot
    /// and a finals slot and already performed, the judge in the published
    /// jury plan.
    async fn add_club_data_and_planning(db: &sqlx::SqlitePool) {
        let (user, club, rider, act, judge) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        let heat_id =
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM heats WHERE name = 'Vorlauf'")
                .fetch_one(db)
                .await
                .unwrap();
        let final_id =
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM finals WHERE name = 'Finale'")
                .fetch_one(db)
                .await
                .unwrap();
        let statements: Vec<sqlx::query::Query<'_, sqlx::Sqlite, _>> = vec![
            sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, 'max@example.org', true, 'Max', '')")
                .bind(user),
            sqlx::query("INSERT INTO clubs (id, name, owner_id) VALUES ($1, 'Club A', $2)")
                .bind(club)
                .bind(user),
            sqlx::query("INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender) VALUES ($1, $2, 'Erika', 'Muster', '2014-01-01T00:00:00Z', 'female')")
                .bind(rider)
                .bind(club),
            sqlx::query("INSERT INTO acts (id, name, act_type, song_file, started_at, ended_at) VALUES ($1, 'Kür', 'single', 'song.mp3', '2027-06-05T10:00:00Z', '2027-06-05T10:04:00Z')").bind(act),
            sqlx::query("INSERT INTO act_participants (act_id, rider_id) VALUES ($1, $2)")
                .bind(act)
                .bind(rider),
            sqlx::query("INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES ($1, $2, 'Max', 'Müller', '1990-01-01T00:00:00Z', true)")
                .bind(judge)
                .bind(club),
            sqlx::query("INSERT INTO judge_registration (judge_id, heat_id, task) VALUES ($1, $2, 'technical')")
                .bind(judge)
                .bind(heat_id),
            sqlx::query("INSERT INTO judge_registration (judge_id, final_id, task) VALUES ($1, $2, 'performance')")
                .bind(judge)
                .bind(final_id),
            sqlx::query("INSERT INTO jury_assignments (judge_id, heat_id, task) VALUES ($1, $2, 'technical')")
                .bind(judge)
                .bind(heat_id),
            sqlx::query("UPDATE competition SET jury_plan_published_at = '2027-01-01T00:00:00Z'"),
            sqlx::query("INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES ((SELECT MIN(id) FROM timeplan), 1, $1)")
                .bind(act),
            sqlx::query("UPDATE final_slots SET act_id = $1 WHERE position = 1").bind(act),
            sqlx::query("INSERT INTO bookings (id, amount, date, club_id) VALUES ($1, 15, '2027-01-01', $2)")
                .bind(uuid::Uuid::now_v7())
                .bind(club),
        ];
        for statement in statements {
            statement.execute(db).await.unwrap();
        }
    }

    #[tokio::test]
    async fn replace_keeps_club_data_and_drops_the_planning() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, &planned_competition(), false, &data.0)
            .await
            .unwrap();
        add_club_data_and_planning(&db).await;

        let replacement = planned_competition()
            .replace("name = \"Cup\"", "name = \"Cup 2\"")
            .replace("Finale", "Endlauf");
        let report = apply_file(&db, &replacement, true, &data.0).await.unwrap();

        let Applied::Replaced(dropped) = report.applied else {
            panic!("{:?}", report.applied);
        };
        assert_eq!(dropped.timeplan_slots, 1);
        assert_eq!(dropped.act_progress, 1);
        assert_eq!(dropped.finals_placements, 1);
        assert_eq!(dropped.jury_assignments, 1);
        assert!(dropped.jury_plan_published);
        let unmatched = vec![RemapRegistration {
            club: "Club A".into(),
            firstname: "Max".into(),
            lastname: "Müller".into(),
            task: JudgeTask::Performance,
            old_jury_unit: "final: Finale".into(),
            jury_unit: String::new(),
        }];
        assert_eq!(dropped.judge_registrations, unmatched);
        let remap_file = dropped.remap_file.expect("a remap file");
        assert!(
            remap_file.starts_with(data.0.join("remap")),
            "{remap_file:?}"
        );
        let name = remap_file.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("jury-registrations-") && name.ends_with(".toml"),
            "{name}"
        );
        let written: RemapFile =
            toml::from_str(&std::fs::read_to_string(&remap_file).unwrap()).unwrap();
        assert_eq!(written.jury_registrations, unmatched);
        assert!(
            report.warnings.iter().any(|w| w.contains("1 timeplan slot")
                && w.contains("the progress of 1 act")
                && w.contains("1 finals placement")
                && w.contains("the published jury plan")
                && w.contains("1 judge registration")),
            "{:?}",
            report.warnings
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains(&remap_file.display().to_string())),
            "{:?}",
            report.warnings
        );

        let competition = Competition::current(&db).await.unwrap().unwrap();
        assert_eq!(
            (competition.id.as_str(), competition.name.as_str()),
            ("cup", "Cup 2")
        );
        for (table, expected) in [
            ("users", 1),
            ("clubs", 1),
            ("rider", 1),
            ("acts", 1),
            ("act_participants", 1),
            ("judge", 1),
            ("bookings", 1),
            ("timeplan_slots", 0),
            ("jury_assignments", 0),
            ("judge_registration", 1),
        ] {
            assert_eq!(count(&db, table).await, expected, "{table}");
        }
        let kept: (String, String) = sqlx::query_as(
            "SELECT view_jury_unit.name, task FROM judge_registration JOIN view_jury_unit ON view_jury_unit.id = unit_id",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(kept, ("Vorlauf".to_string(), "technical".to_string()));
        let progress: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT started_at, ended_at FROM acts")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(progress, (None, None));
        let placements: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM final_slots WHERE act_id IS NOT NULL")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(placements, 0);
        let published: Option<String> =
            sqlx::query_scalar("SELECT jury_plan_published_at FROM competition")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(published, None);
    }

    #[tokio::test]
    async fn replace_keeps_the_database_id_and_warns() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

        let renamed = COMPETITION
            .replace(r#"id = "cup""#, r#"id = "cup-2""#)
            .replace("name = \"Cup\"", "name = \"Cup 2\"");
        let report = apply_file(&db, &renamed, true, &data.0).await.unwrap();
        assert_eq!(report.applied, Applied::Replaced(Dropped::default()));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("id cup-2") && w.contains("keeps cup")),
            "{:?}",
            report.warnings
        );
        assert_eq!(count(&db, "competition").await, 1);
        let competition = Competition::current(&db).await.unwrap().unwrap();
        assert_eq!(
            (competition.id.as_str(), competition.name.as_str()),
            ("cup", "Cup 2")
        );
    }

    #[tokio::test]
    async fn an_invalid_replacement_changes_nothing() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let data = TempDir::new();
        apply_file(&db, &planned_competition(), false, &data.0)
            .await
            .unwrap();
        add_club_data_and_planning(&db).await;

        let invalid = planned_competition()
            .replace("Finale", "Endlauf")
            .replace("category = \"15+\"", "category = \"missing\"");
        let err = apply_file(&db, &invalid, true, &data.0).await.unwrap_err();
        assert!(err.contains("missing"), "{err}");
        assert_eq!(count(&db, "timeplan_slots").await, 1);
        assert_eq!(count(&db, "jury_assignments").await, 1);
        assert_eq!(count(&db, "judge_registration").await, 2);
        assert!(!data.0.join("remap").exists());
    }

    #[tokio::test]
    async fn seeds_empty_database() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        seed_file(&db, COMPETITION).await.unwrap();

        let competition = Competition::current(&db).await.unwrap().unwrap();
        assert_eq!(competition.determining_day, DAY);
        assert_eq!(competition.max_fee_per_rider, None);
        let categories: Vec<(String, i64)> =
            sqlx::query_as(r#"SELECT name, "order" FROM categories ORDER BY "order""#)
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(
            categories,
            vec![("U15".to_string(), 1), ("15+".to_string(), 2)]
        );
        assert_eq!(count(&db, "timeplan").await, 3);
    }

    #[tokio::test]
    async fn invalid_competition_dates_write_nothing() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let broken = COMPETITION.replace(
            "registration_end = 2027-04-30T23:59:59+02:00",
            "registration_end = 2027-12-20T23:59:59+01:00",
        );
        let err = seed_file(&db, &broken).await.unwrap_err();
        assert!(err.contains("registration_end"), "{err}");
        assert_eq!(count(&db, "competition").await, 0);
    }

    #[tokio::test]
    async fn invalid_config_writes_nothing() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let invalid = COMPETITION.replace("category = \"15+\"", "category = \"missing\"");
        let err = seed_file(&db, &invalid).await.unwrap_err();
        assert!(err.contains("missing"), "{err}");
        assert_eq!(count(&db, "competition").await, 0);
        assert_eq!(count(&db, "categories").await, 0);
        assert_eq!(count(&db, "timeplan").await, 0);
    }

    /// The competition with heats and finals appended.
    fn with_heats(heats_and_finals: &str) -> String {
        format!(
            r#"{COMPETITION}
        [[categories]]
        name = "PU15"
        act_type = "pair"
        under_age = 15

        [[categories]]
        name = "M15"
        act_type = "single"
        single_male = true
        under_age = 15

        [[categories]]
        name = "KG"
        act_type = "group_small"

        {heats_and_finals}
        "#
        )
    }

    #[tokio::test]
    async fn heats_are_seeded_with_their_categories() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let file = with_heats(
            r#"
            [[heats]]
            name = "Junior"
            description = "Junior Expert"
            categories = ["U15"]

            [[heats]]
            name = "Expert"
            categories = ["15+"]

            [[finals]]
            name = "Finale"
            act_type = "single"
            slots = [{ heat = "Junior", count = 2 }, { heat = "Expert", count = 1 }]
            "#,
        );
        seed_file(&db, &file).await.unwrap();
        let heats: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT name, description FROM heats ORDER BY name")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(
            heats,
            [
                ("Expert".to_string(), None),
                ("Junior".to_string(), Some("Junior Expert".to_string())),
            ]
        );
        let linked: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT categories.name, heats.name FROM categories \
                 LEFT JOIN heats ON heats.id = categories.heat_id ORDER BY categories.\"order\"",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            linked,
            [
                ("U15".to_string(), Some("Junior".to_string())),
                ("15+".to_string(), Some("Expert".to_string())),
                ("PU15".to_string(), None),
                ("M15".to_string(), None),
                ("KG".to_string(), None),
            ]
        );
        let slots: Vec<String> = sqlx::query_scalar(
            "SELECT heats.name FROM final_slots \
                 JOIN heats ON heats.id = final_slots.heat_id ORDER BY position",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(slots, ["Junior", "Junior", "Expert"]);
    }

    const NAMED_HEATS: &str = r#"
        [[heats]]
        name = "Einzel – Junior (U15)"
        code = "J"
        categories = ["U15"]

        [[heats]]
        name = "Einzel – Expert (15+)"
        categories = ["15+"]

        [[finals]]
        name = "Finale Einzel"
        code = "FE"
        act_type = "single"
        slots = [{ heat = "J", count = 2 }, { heat = "Einzel – Expert (15+)", count = 1 }]

        [[timeplan]]
        final = "FE"
    "#;

    #[tokio::test]
    async fn heats_and_finals_take_name_and_code_and_files_refer_to_the_code() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        seed_file(&db, &with_heats(NAMED_HEATS)).await.unwrap();

        let heats: Vec<(String, String)> =
            sqlx::query_as("SELECT code, name FROM heats ORDER BY code")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(
            heats,
            [
                // Without a code, the name is the code.
                (
                    "Einzel – Expert (15+)".to_string(),
                    "Einzel – Expert (15+)".to_string()
                ),
                ("J".to_string(), "Einzel – Junior (U15)".to_string()),
            ]
        );
        let finals: Vec<(String, String)> = sqlx::query_as("SELECT code, name FROM finals")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(finals, [("FE".to_string(), "Finale Einzel".to_string())]);
        let planned: Vec<String> = sqlx::query_scalar(
            "SELECT finals.name FROM timeplan JOIN finals ON finals.id = timeplan.final_id",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(planned, ["Finale Einzel"]);
    }

    #[tokio::test]
    async fn references_name_the_code_not_the_readable_name() {
        for (from, to, message) in [
            (
                "heat = \"J\"",
                "heat = \"Einzel – Junior (U15)\"",
                "Einzel – Junior (U15)",
            ),
            (
                "final = \"FE\"",
                "final = \"Finale Einzel\"",
                "Finale Einzel",
            ),
        ] {
            let db = crate::reloadable_sqlite::in_memory_migrated().await;
            let file = with_heats(&NAMED_HEATS.replace(from, to));
            let err = seed_file(&db, &file).await.unwrap_err();
            assert!(err.contains(message), "{err}");
        }
    }

    #[tokio::test]
    async fn codes_must_be_unique() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let file = with_heats(&NAMED_HEATS.replace(
            "name = \"Einzel – Expert (15+)\"",
            "name = \"Einzel – Expert (15+)\"\ncode = \"J\"",
        ));
        let err = seed_file(&db, &file).await.unwrap_err();
        assert!(err.contains("Heat J"), "{err}");
    }

    #[tokio::test]
    async fn heats_take_their_threshold_and_skip_from_the_file() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let file = with_heats(
            r#"
            [[heats]]
            name = "Junior"
            categories = ["U15"]

            [[heats]]
            name = "Expert"
            categories = ["15+"]

            [[heats]]
            name = "Paare"
            categories = ["PU15"]
            threshold = 3
            skip = true
            "#,
        );
        seed_file(&db, &file).await.unwrap();
        let heats: Vec<(String, i64, bool)> =
            sqlx::query_as("SELECT name, threshold, skip FROM heats ORDER BY name")
                .fetch_all(&db)
                .await
                .unwrap();
        // Without a threshold: 6 for U15 and younger, otherwise 5.
        assert_eq!(
            heats,
            [
                ("Expert".to_string(), 5, false),
                ("Junior".to_string(), 6, false),
                ("Paare".to_string(), 3, true),
            ]
        );
    }

    #[tokio::test]
    async fn heats_of_groups_skip_their_vorlauf_unless_the_file_says_otherwise() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let file = with_heats(
            r#"
            [[heats]]
            name = "Kleingruppe"
            categories = ["KG"]

            [[finals]]
            name = "Finale KG"
            act_type = "group_small"
            slots = [{ heat = "Kleingruppe", count = 1 }]
            "#,
        );
        seed_file(&db, &file).await.unwrap();
        let skip: bool = sqlx::query_scalar("SELECT skip FROM heats WHERE name = 'Kleingruppe'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(skip);

        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        seed_file(
            &db,
            &file.replace(
                "categories = [\"KG\"]",
                "categories = [\"KG\"]\nskip = false",
            ),
        )
        .await
        .unwrap();
        let skip: bool = sqlx::query_scalar("SELECT skip FROM heats WHERE name = 'Kleingruppe'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(!skip);
        // The export gives the default back as no skip key.
        let export = crate::export::export(&mut db.acquire().await.unwrap())
            .await
            .unwrap()
            .unwrap();
        let heat = export
            .file
            .heats
            .iter()
            .find(|h| h.name == "Kleingruppe")
            .unwrap();
        assert_eq!(heat.skip, Some(false));
    }

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

    #[tokio::test]
    async fn the_import_checks_heats_and_names_the_offender() {
        let heat = |name: &str, categories: &str| {
            format!("[[heats]]\nname = \"{name}\"\ncategories = [{categories}]\n")
        };
        let single_final = |heat: &str| {
            format!(
                "[[finals]]\nname = \"Finale\"\nact_type = \"single\"\nslots = [{{ heat = \"{heat}\", count = 1 }}]\n"
            )
        };
        for (file, message) in [
            (heat(" ", "\"U15\""), "empty name"),
            (heat("A", "\"U15\"") + &heat("A", "\"15+\""), "Heat A"),
            (heat("A", "\"U99\""), "U99"),
            // One act type and gender.
            (heat("A", "\"U15\", \"PU15\""), "PU15"),
            (heat("A", "\"U15\", \"M15\""), "M15"),
            // A category belongs to at most one heat.
            (heat("A", "\"U15\"") + &heat("B", "\"U15\""), "U15"),
            (heat("A", "\"U15\"") + "threshold = -1\n", "Heat A"),
            // A heat fills the slots of at most one final.
            (
                heat("A", "\"U15\"")
                    + &single_final("A")
                    + &single_final("A").replace("\"Finale\"", "\"Finale 2\""),
                "Final Finale 2",
            ),
            // A slot group names a heat of the file, of the final's discipline.
            (single_final("Fehlt"), "Fehlt"),
            (heat("A", "\"PU15\"") + &single_final("A"), "Final Finale"),
            (
                heat("A", "\"U15\"")
                    + "[[finals]]\nname = \"Gruppe\"\nact_type = \"group_small\"\nslots = [{ heat = \"A\", count = 1 }]\n",
                "Final Gruppe",
            ),
        ] {
            let db = crate::reloadable_sqlite::in_memory_migrated().await;
            let err = seed_file(&db, &with_heats(&file)).await.unwrap_err();
            assert!(err.contains(message), "{message}: {err}");
            assert_eq!(count(&db, "competition").await, 0);
        }
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

    #[tokio::test]
    async fn age_uses_determining_day_of_competition() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        seed_file(&db, COMPETITION).await.unwrap();
        let age: f64 = sqlx::query_scalar(
            "SELECT (julianday((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) - julianday('2012-06-01')) / 365.2425",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert!((age - 15.0).abs() < 0.01, "{age}");
    }
}
