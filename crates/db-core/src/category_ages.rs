//! The age rules of categories: the birthday bounds that follow from
//! `min_age` / `under_age` and the determining day, and the coverage check
//! over them (AGE-2). Pure; the application's competition import and the
//! category commands of the backends share it.

use std::collections::{BTreeMap, HashSet};

use time::{Date, Month};

use crate::act_type::ActType;

/// A category from the config with its birthday bounds derived from the
/// determining day; they are never stored (#256).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCategory {
    pub name: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    /// `None` for an open end at the bottom, e.g. "70+" has no `under_age`.
    pub min_age: Option<f64>,
    /// `None` for an open end at the top, e.g. "U11" has no `min_age`.
    pub under_age: Option<f64>,
    /// Inclusive, from `under_age`; `None` for no lower bound.
    pub from_birthday: Option<Date>,
    /// Exclusive, from `min_age`; `None` for no upper bound.
    pub to_birthday: Option<Date>,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
}

/// The date `years` years before `day`. Fractions are rounded to whole months,
/// so `9.5` means 9 years and 6 months. Days that don't exist in the target
/// month (e.g. 29 February) move to the month's last day.
///
/// ```
/// use db_core::category_ages::years_before;
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

/// Checks a category's ages and derives its birthday bounds from
/// `determining_day` (AGE-2): `(from_birthday, to_birthday)`, the first
/// inclusive, the second exclusive, `None` for an open end.
///
/// A rider whose N-th birthday is on the determining day remains in the
/// younger category, so `under_age = N` gives
/// `from_birthday = determining_day - N years`. `min_age = M` gives
/// `to_birthday = determining_day - M years`.
pub fn birthday_bounds(
    name: &str,
    min_age: Option<f64>,
    under_age: Option<f64>,
    determining_day: Date,
) -> Result<(Option<Date>, Option<Date>), String> {
    for age in [min_age, under_age].into_iter().flatten() {
        if age < 0.0 || !age.is_finite() {
            return Err(format!("Category {name}: ages must not be negative."));
        }
    }
    if let (Some(min), Some(under)) = (min_age, under_age)
        && min >= under
    {
        return Err(format!(
            "Category {name}: min_age ({min}) must be lower than under_age ({under})."
        ));
    }
    let from_birthday = under_age.map(|under| years_before(determining_day, under));
    let to_birthday = min_age.map(|min| years_before(determining_day, min));
    if let (Some(from), Some(to)) = (from_birthday, to_birthday)
        && from >= to
    {
        return Err(format!(
            "Category {name}: min_age and under_age must be at least a month apart."
        ));
    }
    Ok((from_birthday, to_birthday))
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

    // Open ends as the earliest and latest possible birthday.
    let from = |c: &ResolvedCategory| c.from_birthday.unwrap_or(Date::MIN);
    let to = |c: &ResolvedCategory| c.to_birthday.unwrap_or(Date::MAX);
    let mut warnings = Vec::new();
    for ((act_type, single_male), mut group) in groups {
        group.sort_by_key(|c| from(c));
        let label = format!("{act_type}{}", if single_male { ", male" } else { "" });
        if let Some(first) = group[0].from_birthday {
            warnings.push(format!(
                "Categories ({label}): riders born before {first} match no category."
            ));
        }
        for pair in group.windows(2) {
            let (current, next) = (pair[0], pair[1]);
            if from(next) < to(current) {
                warnings.push(format!(
                    "Categories ({label}): {} and {} overlap.",
                    current.name, next.name
                ));
            } else if from(next) > to(current) {
                warnings.push(format!(
                    "Categories ({label}): riders born from {} to before {} match neither {} nor {}.",
                    to(current),
                    from(next),
                    current.name,
                    next.name
                ));
            }
        }
        let last = group
            .iter()
            .max_by_key(|c| to(c))
            .expect("group isn't empty");
        if let Some(last) = last.to_birthday {
            warnings.push(format!(
                "Categories ({label}): riders born on or after {last} match no category."
            ));
        }
    }
    warnings
}

/// A stored category as far as its ages are concerned.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCategoryAges {
    pub name: String,
    pub act_type: ActType,
    pub single_male: bool,
    pub min_age: Option<f64>,
    pub under_age: Option<f64>,
}

/// [`coverage_warnings`] for stored categories (in category order), with the
/// bounds derived from the competition's determining day: the check the
/// import runs, for edits in the admin UI.
pub fn stored_coverage_warnings(
    categories: &[StoredCategoryAges],
    determining_day: Date,
) -> Result<Vec<String>, String> {
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
            description: None,
            act_type: category.act_type,
            single_male: category.single_male,
            min_age: category.min_age,
            under_age: category.under_age,
            from_birthday,
            to_birthday,
            einfahrzeit_seconds: 0,
            act_duration_seconds: 0,
            judge_duration_seconds: 0,
        });
    }
    Ok(coverage_warnings(&resolved))
}
