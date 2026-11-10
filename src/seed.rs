//! Applies the competition file: the competition, categories, finals,
//! timeplan and jury categories.
//!
//! The database is the source of the competition (ADR 0005), the file only
//! an import: it's applied when the database has no competition yet, or on
//! purpose with `--replace-competition` or the admin UI's import. See
//! [`apply`] and [`preview`].

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use time::{Date, Month, OffsetDateTime, macros::format_description};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    act_type::ActType,
    colors,
    competition::{Competition, upsert_competition},
    config::{
        Branding, CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig, Settings,
        SponsorConfig, TimeplanEntryConfig,
    },
    finals::{self, SlotGroup},
    images::{self, Image, ImageUrl, Slot},
    jury::JudgeTask,
    logo::{Logo, SvgLogo},
    reloadable_sqlite::begin_write,
    roles::Role,
    timeplan::EntryKind,
};

/// Lower birthday bound for categories without a maximum age.
pub const OPEN_FROM_BIRTHDAY: Date = time::macros::date!(1900 - 01 - 01);
/// Upper birthday bound for categories without a minimum age.
pub const OPEN_TO_BIRTHDAY: Date = time::macros::date!(9999 - 12 - 31);

/// A category from the config with its birthday bounds calculated.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCategory {
    pub name: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    /// Inclusive.
    pub from_birthday: Date,
    /// Exclusive.
    pub to_birthday: Date,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
}

/// The date `years` years before `day`. Fractions are rounded to whole months,
/// so `9.5` means 9 years and 6 months. Days that don't exist in the target
/// month (e.g. 29 February) move to the month's last day.
///
/// ```
/// use einrad_server::seed::years_before;
/// use time::macros::date;
///
/// assert_eq!(years_before(date!(2027 - 06 - 01), 15.0), date!(2012 - 06 - 01));
/// assert_eq!(years_before(date!(2027 - 06 - 01), 9.5), date!(2017 - 12 - 01));
/// assert_eq!(years_before(date!(2028 - 02 - 29), 1.0), date!(2027 - 02 - 28));
/// ```
pub fn years_before(day: Date, years: f64) -> Date {
    let months = (years * 12.0).round() as i32;
    let total = day.year() * 12 + (day.month() as i32 - 1) - months;
    let year = total.div_euclid(12);
    let month = Month::try_from((total.rem_euclid(12) + 1) as u8).expect("valid month");
    let mut day_of_month = day.day();
    loop {
        if let Ok(date) = Date::from_calendar_date(year, month, day_of_month) {
            return date;
        }
        day_of_month -= 1;
    }
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
/// and validates them.
///
/// A rider whose N-th birthday is on the determining day remains in the
/// younger category, so `under_age = N` gives
/// `from_birthday = determining_day - N years`. `min_age = M` gives
/// `to_birthday = determining_day - M years` (exclusive).
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
        for age in [category.min_age, category.under_age].into_iter().flatten() {
            if age < 0.0 || !age.is_finite() {
                return Err(format!("Category {name}: ages must not be negative."));
            }
        }
        if let (Some(min), Some(under)) = (category.min_age, category.under_age)
            && min >= under
        {
            return Err(format!(
                "Category {name}: min_age ({min}) must be lower than under_age ({under})."
            ));
        }

        let from_birthday = match (category.from_birthday, category.under_age) {
            (Some(date), _) => date,
            (None, Some(under)) => years_before(determining_day, under),
            (None, None) => OPEN_FROM_BIRTHDAY,
        };
        let to_birthday = match (category.to_birthday, category.min_age) {
            (Some(date), _) => date,
            (None, Some(min)) => years_before(determining_day, min),
            (None, None) => OPEN_TO_BIRTHDAY,
        };
        if from_birthday >= to_birthday {
            return Err(format!(
                "Category {name}: from_birthday ({from_birthday}) must be before to_birthday ({to_birthday})."
            ));
        }

        resolved.push(ResolvedCategory {
            name: name.clone(),
            description: category.description.clone(),
            act_type: category.act_type,
            single_male: category.single_male,
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
    pub name: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    pub slots: Vec<SlotGroup>,
    pub categories: Vec<String>,
}

/// Checks the finals of the config and converts their durations.
pub fn resolve_finals(finals: &[FinalConfig]) -> Result<Vec<ResolvedFinal>, String> {
    let mut names = HashSet::new();
    finals
        .iter()
        .map(|config| {
            let name = config.name.trim();
            if name.is_empty() {
                return Err("A final has an empty name.".to_string());
            }
            if !names.insert(name.to_string()) {
                return Err(format!("Final {name} is defined more than once."));
            }
            let slots = finals::normalize_slot_groups(&config.slots).map_err(|_| {
                format!(
                    "Final {name}: slots need at least one slot, each group a count of at least 1, at most {} in all.",
                    finals::MAX_SLOTS
                )
            })?;
            Ok(ResolvedFinal {
                name: name.to_string(),
                description: config.description.clone(),
                act_type: config.act_type,
                single_male: config.single_male,
                einfahrzeit_seconds: minutes_to_seconds(
                    config.einfahrzeit_minutes,
                    "einfahrzeit_minutes",
                    name,
                )?,
                act_duration_seconds: minutes_to_seconds(
                    config.act_duration_minutes,
                    "act_duration_minutes",
                    name,
                )?,
                judge_duration_seconds: minutes_to_seconds(
                    config.judge_duration_minutes,
                    "judge_duration_minutes",
                    name,
                )?,
                slots,
                categories: finals::normalize_categories(&config.categories),
            })
        })
        .collect()
}

/// Finds overlapping or missing birthday ranges among categories that compete
/// for the same riders (same act type and gender filter).
pub fn coverage_warnings(categories: &[ResolvedCategory]) -> Vec<String> {
    let mut groups: BTreeMap<(&str, bool), Vec<&ResolvedCategory>> = BTreeMap::new();
    for category in categories {
        groups
            .entry((category.act_type.as_str(), category.single_male))
            .or_default()
            .push(category);
    }

    let mut warnings = Vec::new();
    for ((act_type, single_male), mut group) in groups {
        group.sort_by_key(|c| c.from_birthday);
        let label = format!("{act_type}{}", if single_male { ", male" } else { "" });
        if group[0].from_birthday > OPEN_FROM_BIRTHDAY {
            warnings.push(format!(
                "Categories ({label}): riders born before {} match no category.",
                group[0].from_birthday
            ));
        }
        for pair in group.windows(2) {
            let (current, next) = (pair[0], pair[1]);
            if next.from_birthday < current.to_birthday {
                warnings.push(format!(
                    "Categories ({label}): {} and {} overlap.",
                    current.name, next.name
                ));
            } else if next.from_birthday > current.to_birthday {
                warnings.push(format!(
                    "Categories ({label}): riders born from {} to before {} match neither {} nor {}.",
                    current.to_birthday, next.from_birthday, current.name, next.name
                ));
            }
        }
        let last = group
            .iter()
            .max_by_key(|c| c.to_birthday)
            .expect("group isn't empty");
        if last.to_birthday < OPEN_TO_BIRTHDAY {
            warnings.push(format!(
                "Categories ({label}): riders born on or after {} match no category.",
                last.to_birthday
            ));
        }
    }
    warnings
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
        entry.category.as_deref().or(entry.final_name.as_deref()),
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

fn midnight_utc(date: Date) -> OffsetDateTime {
    date.midnight().assume_utc()
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
    Finals,
    Timeplan,
    JuryCategories,
}

impl Part {
    /// For warnings, e.g. "the categories".
    fn describe(self) -> &'static str {
        match self {
            Part::Settings => "the competition settings",
            Part::Images => "the logo, break image and sponsors",
            Part::Colors => "the colors",
            Part::Categories => "the categories",
            Part::Finals => "the finals",
            Part::Timeplan => "the timeplan",
            Part::JuryCategories => "the jury categories",
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
    /// Judge registrations whose jury category has no namesake in the file.
    pub judge_registrations: Vec<RemapRegistration>,
    /// Where they were written, to fill in `jury_category` and apply again.
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
                "judge registration without a matching jury category",
                "judge registrations without a matching jury category",
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// A judge registration a replace dropped, as in the remap file. Filling in
/// `jury_category` says which new jury category the judge belongs to.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RemapRegistration {
    pub club: String,
    pub firstname: String,
    pub lastname: String,
    pub task: JudgeTask,
    pub old_jury_category: String,
    /// Empty until filled in.
    pub jury_category: String,
}

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

/// The competition the server starts with, or `None` for setup mode:
/// without a competition the server starts as long as an admin exists, who
/// imports one in the admin UI. Without either it refuses to start, as
/// nobody could import one.
pub async fn startup_competition(db: &sqlx::SqlitePool) -> Result<Option<Competition>, String> {
    if let Some(competition) = Competition::current(db).await.map_err(db_error)? {
        return Ok(Some(competition));
    }
    let admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = ?"#,
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
    apply(&db, Some(&file), false, &std::env::temp_dir())
        .await
        .map(|_| ())
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
    finals: Vec<String>,
    timeplan: Vec<String>,
    jury_categories: Vec<String>,
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
                r#"SELECT json_array(name, description, from_birthday, to_birthday, act_type,
                    is_single_male, "order", einfahrzeit_seconds, act_duration_seconds,
                    judge_duration_seconds)
                FROM categories ORDER BY "order", name"#,
            )
            .await?,
            finals: rows(
                r#"SELECT json_array(name, description, act_type, is_single_male, "order",
                    einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds,
                    (SELECT json_group_array(json_array(group_label, number))
                        FROM (SELECT group_label, number FROM final_slots
                            WHERE final_name = finals.name ORDER BY position)),
                    (SELECT json_group_array(category)
                        FROM (SELECT category FROM final_categories
                            WHERE final_name = finals.name ORDER BY category)))
                FROM finals ORDER BY "order", name"#,
            )
            .await?,
            timeplan: rows(
                r#"SELECT json_array(earliest_start_time, duration_seconds, label, category,
                    final_name, kind, show_photos, internal, einfahrzeit_seconds)
                FROM timeplan ORDER BY position, id"#,
            )
            .await?,
            jury_categories: rows(
                r#"SELECT json_array(name, deployments, day, needed_technical,
                    needed_performance, needed_dismounts)
                FROM jury_categories ORDER BY sort_order"#,
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
            (self.finals != other.finals, Part::Finals),
            (self.timeplan != other.timeplan, Part::Timeplan),
            (
                self.jury_categories != other.jury_categories,
                Part::JuryCategories,
            ),
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
                " They are listed in {}, with an empty jury_category to fill in.",
                path.display()
            )
        })
        .unwrap_or_default();
    Some(format!(
        "Applying the competition file dropped {}.{remap}",
        summary.join(", ")
    ))
}

/// Writes the file's competition with `id`, its categories, finals, timeplan
/// and jury categories, replacing what the database has (ADR 0005): the
/// planning that depends on them is dropped, club data is kept. Judge
/// registrations follow their jury category by name; the others are returned
/// for the remap file.
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
            judge.id as "judge_id!: Uuid",
            clubs.name as "club!",
            judge.firstname as "firstname!",
            judge.lastname as "lastname!",
            judge_registration.task as "task!: JudgeTask",
            jury_categories.name as "jury_category!"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN clubs ON clubs.id = judge.club_id
        JOIN jury_categories ON jury_categories.id = judge_registration.jury_category_id
        ORDER BY clubs.name, judge.lastname, judge.firstname, jury_categories.sort_order, judge_registration.task
        "#
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_error)?;

    for table in [
        "jury_assignments",
        "judge_registration",
        "jury_categories",
        "timeplan_slots",
        "timeplan",
        "final_categories",
        "final_slots",
        "finals",
        "categories",
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
    sqlx::query!("DELETE FROM competition WHERE id != ?", competition.id)
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
    let jury_categories = &file.jury_categories;

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
            let from_birthday = midnight_utc(category.from_birthday);
            let to_birthday = midnight_utc(category.to_birthday);
            sqlx::query!(
                r#"
                INSERT INTO categories (
                    name, description, from_birthday, to_birthday, act_type,
                    is_single_male, "order", einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                category.name,
                category.description,
                from_birthday,
                to_birthday,
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

    if !finals.is_empty() {
        let resolved = resolve_finals(finals)?;
        for (order, item) in (1_i64..).zip(&resolved) {
            sqlx::query!(
                r#"
                INSERT INTO finals (
                    name, description, act_type, is_single_male, "order",
                    einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                item.name,
                item.description,
                item.act_type,
                item.single_male,
                order,
                item.einfahrzeit_seconds,
                item.act_duration_seconds,
                item.judge_duration_seconds,
            )
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
            finals::replace_slots(conn, &item.name, &item.slots)
                .await
                .map_err(db_error)?;
            if finals::check_categories(conn, &item.categories)
                .await
                .is_err()
            {
                return Err(format!(
                    "Final {}: categories must name categories of the config.",
                    item.name
                ));
            }
            finals::set_categories(conn, &item.name, &item.categories)
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
        let final_names: Vec<String> = sqlx::query_scalar!(r#"SELECT name as "name!" FROM finals"#)
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
            sqlx::query!(
                r#"
                INSERT INTO timeplan (
                    earliest_start_time, duration_seconds, label, category, final_name, kind,
                    show_photos, internal, position, einfahrzeit_seconds
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                entry.earliest_start,
                duration_seconds,
                entry.label,
                entry.category,
                entry.final_name,
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

    if !jury_categories.is_empty() {
        let competition = Competition::current(&mut *conn).await.map_err(db_error)?;
        for category in jury_categories {
            let fields = crate::jury::JuryCategoryFields {
                name: category.name.clone(),
                deployments: category.deployments,
                day: category.day,
                needed_technical: category.needed_technical,
                needed_performance: category.needed_performance,
                needed_dismounts: category.needed_dismounts,
            }
            .normalize(competition.as_ref())
            .map_err(|e| format!("[[jury_categories]] {:?}: {e}", category.name))?;
            crate::jury::insert_category(conn, &fields)
                .await
                .map_err(|e| format!("[[jury_categories]] {:?}: {e:?}", category.name))?;
        }
        info!(
            "Seeded {} jury categories from the competition file",
            jury_categories.len()
        );
    }

    for registration in registrations {
        let category = sqlx::query_scalar!(
            r#"SELECT id as "id!: Uuid" FROM jury_categories WHERE name = ?"#,
            registration.jury_category
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_error)?;
        match category {
            Some(category) => {
                sqlx::query!(
                    "INSERT INTO judge_registration (judge_id, jury_category_id, task) VALUES (?, ?, ?)",
                    registration.judge_id,
                    category,
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
                old_jury_category: registration.jury_category,
                jury_category: String::new(),
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
        "# Judge registrations whose jury category the competition file dropped.\n\
         # Fill in jury_category with the jury category of the new competition\n\
         # each judge belongs to.\n\n{}",
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
        assert_eq!(resolved[0].from_birthday, date!(2012 - 06 - 01));
        assert_eq!(resolved[0].to_birthday, OPEN_TO_BIRTHDAY);
        assert_eq!(resolved[1].from_birthday, OPEN_FROM_BIRTHDAY);
        assert_eq!(resolved[1].to_birthday, date!(2012 - 06 - 01));
        assert!(coverage_warnings(&resolved).is_empty());
    }

    #[test]
    fn half_years_are_supported() {
        let config = categories(
            &(category("U9,5", None, Some(9.5)) + &category("U11", Some(9.5), Some(11.0))),
        );
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].from_birthday, date!(2017 - 12 - 01));
        assert_eq!(resolved[1].from_birthday, date!(2016 - 06 - 01));
        assert_eq!(resolved[1].to_birthday, date!(2017 - 12 - 01));
    }

    #[test]
    fn explicit_birthdays_override_ages() {
        let config = categories(
            "[[categories]]\nname = \"X\"\nact_type = \"pair\"\nunder_age = 15\nfrom_birthday = 2000-01-01\n",
        );
        let resolved = resolve_categories(&config, DAY).unwrap();
        assert_eq!(resolved[0].from_birthday, date!(2000 - 01 - 01));
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
                "name = \"Block B\"",
                "name = \"Block B\"\n        deployments = 1",
                "the jury categories",
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

    /// [`COMPETITION`] with a final and two jury categories.
    fn planned_competition() -> String {
        format!(
            r#"{COMPETITION}
        [[timeplan]]
        final = "Finale"

        [[finals]]
        name = "Finale"
        act_type = "single"
        slots = [{{ label = "U15", count = 2 }}]
        categories = ["U15"]

        [[jury_categories]]
        name = "Block A"

        [[jury_categories]]
        name = "Block B"
        "#
        )
    }

    /// A club with a rider, an act, a judge registered for both jury
    /// categories and a booking, plus planning: the act in a timeplan slot
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
        let block = |name: &'static str| {
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM jury_categories WHERE name = ?")
                .bind(name)
                .fetch_one(db)
        };
        let (block_a, block_b) = (
            block("Block A").await.unwrap(),
            block("Block B").await.unwrap(),
        );
        let statements: Vec<sqlx::query::Query<'_, sqlx::Sqlite, _>> = vec![
            sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES (?, 'max@example.org', true, 'Max', '')")
                .bind(user),
            sqlx::query("INSERT INTO clubs (id, name, owner_id) VALUES (?, 'Club A', ?)")
                .bind(club)
                .bind(user),
            sqlx::query("INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender) VALUES (?, ?, 'Erika', 'Muster', '2014-01-01T00:00:00Z', 'female')")
                .bind(rider)
                .bind(club),
            sqlx::query("INSERT INTO acts (id, name, act_type, song_file, started_at, ended_at) VALUES (?, 'Kür', 'single', 'song.mp3', '2027-06-05T10:00:00Z', '2027-06-05T10:04:00Z')").bind(act),
            sqlx::query("INSERT INTO act_participants (act_id, rider_id) VALUES (?, ?)")
                .bind(act)
                .bind(rider),
            sqlx::query("INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES (?, ?, 'Max', 'Müller', '1990-01-01T00:00:00Z', true)")
                .bind(judge)
                .bind(club),
            sqlx::query("INSERT INTO judge_registration (judge_id, jury_category_id, task) VALUES (?, ?, 'technical')")
                .bind(judge)
                .bind(block_a),
            sqlx::query("INSERT INTO judge_registration (judge_id, jury_category_id, task) VALUES (?, ?, 'performance')")
                .bind(judge)
                .bind(block_b),
            sqlx::query("INSERT INTO jury_assignments (judge_id, jury_category_id, task) VALUES (?, ?, 'technical')")
                .bind(judge)
                .bind(block_a),
            sqlx::query("UPDATE competition SET jury_plan_published_at = '2027-01-01T00:00:00Z'"),
            sqlx::query("INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES ((SELECT MIN(id) FROM timeplan), 1, ?)")
                .bind(act),
            sqlx::query("UPDATE final_slots SET act_id = ? WHERE position = 1").bind(act),
            sqlx::query("INSERT INTO bookings (id, amount, date, club_id) VALUES (?, 15, '2027-01-01', ?)")
                .bind(uuid::Uuid::new_v4())
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
            .replace("Block B", "Block C");
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
            old_jury_category: "Block B".into(),
            jury_category: String::new(),
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
            "SELECT jury_categories.name, task FROM judge_registration JOIN jury_categories ON jury_categories.id = jury_category_id",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(kept, ("Block A".to_string(), "technical".to_string()));
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
            .replace("Block B", "Block C")
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
