//! The jury registration (#225): types and rules of jury units, judges and
//! the Juryplan, plus the [`JuryRepo`] trait. The SQL that counts a club's
//! acts and judges lives in the backend.

use time::{Date, OffsetDateTime};

use crate::{
    DbError,
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    ids::{ClubId, FinalId, HeatId, JudgeId, JuryUnitId},
    people,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// The deployments a judge counts for their club: jury units with a
/// technical or performance registration, at most their maximum; untrained
/// people count none.
///
/// ```
/// use db_core::jury::counted_deployments;
///
/// assert_eq!(counted_deployments(true, 6, None), 6);
/// assert_eq!(counted_deployments(true, 6, Some(2)), 2);
/// assert_eq!(counted_deployments(false, 6, None), 0);
/// ```
#[must_use]
pub fn counted_deployments(is_trained: bool, registered: i64, max: Option<i64>) -> i64 {
    if !is_trained {
        return 0;
    }
    max.map_or(registered, |max| registered.min(max))
}

/// Jury deployments (Einsätze) a club must provide: `per_two_acts` for every
/// started two Küren (Jury-Konzept, Tabelle 1).
///
/// ```
/// use db_core::jury::required_deployments;
///
/// assert_eq!(required_deployments(0, 4), 0);
/// assert_eq!(required_deployments(1, 4), 4);
/// assert_eq!(required_deployments(2, 4), 4);
/// assert_eq!(required_deployments(3, 4), 8);
/// assert_eq!(required_deployments(32, 4), 64);
/// ```
#[must_use]
pub fn required_deployments(acts: i64, per_two_acts: i64) -> i64 {
    (acts + 1) / 2 * per_two_acts
}

/// A club's jury obligation.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct JuryObligation {
    /// Küren that count: those charged a start fee, for the owner club.
    pub acts: i64,
    pub required: i64,
    /// What the club's trained judges registered for as technical or
    /// performance judges.
    pub registered: i64,
    /// What an admin credits instead, e.g. after a late cancellation.
    pub credited_override: Option<i64>,
    /// `credited_override`, or `registered` without one.
    pub credited: i64,
    pub missing: i64,
    /// For the missing deployments, in €.
    pub fee: f64,
}

/// Which kind of jury unit: a heat's Vorlauf or a final.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum JuryUnitKind {
    /// The Vorlauf of a heat.
    Heat,
    /// A final.
    Final,
}

impl JuryUnitKind {
    /// How the remap file names the kind.
    fn label(self) -> &'static str {
        match self {
            JuryUnitKind::Heat => "heat",
            JuryUnitKind::Final => "final",
        }
    }
}

/// How a remap file names a jury unit: `heat: E EW` or `final: Finale EW`.
pub fn unit_label(kind: JuryUnitKind, name: &str) -> String {
    format!("{}: {name}", kind.label())
}

/// The kind and name of a unit label (see [`unit_label`]).
pub fn parse_unit_label(label: &str) -> Option<(JuryUnitKind, &str)> {
    let (kind, name) = label.trim().split_once(':')?;
    let kind = match kind.trim() {
        "heat" => JuryUnitKind::Heat,
        "final" => JuryUnitKind::Final,
        _ => return None,
    };
    Some((kind, name.trim())).filter(|(_, name)| !name.is_empty())
}

/// What judges register for and get assigned to: the Vorlauf of a heat or a
/// final (ADR 0016). Its settings live on the heat or the final.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct JuryUnit {
    /// The heat's or the final's id.
    pub id: JuryUnitId,
    pub kind: JuryUnitKind,
    /// The heat's or the final's readable name; renaming it keeps the
    /// registrations. Judges and the jury lead read this one.
    pub name: String,
    /// The heat's or the final's short code.
    pub code: String,
    pub description: Option<String>,
    /// Judges can register for it: its setting, and never for a skipped
    /// Vorlauf.
    pub registrable: bool,
    /// The jury lead's setting alone, which a skipped Vorlauf overrides.
    pub registrable_setting: bool,
    /// The heat's Vorlauf is skipped ("Vorlauf entfällt"). Registrations it
    /// already has are flagged for the jury lead to move.
    pub skipped: bool,
    /// Jury deployments (Einsätze) it counts for.
    pub deployments: i64,
    /// Judges it needs per task.
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
    /// The discipline: the act type of a heat's categories (the first one's
    /// if they differ, else the final it feeds) or of a final. `None` for a
    /// heat without categories that feeds no final.
    pub act_type: Option<ActType>,
    /// The final a heat feeds (one at most); `None` for a final and for a
    /// heat that feeds none.
    pub final_id: Option<FinalId>,
    /// The day it runs, derived from the timeplan; `None` while it isn't
    /// planned in it.
    #[serde(with = "iso_date::option")]
    #[schema(value_type = Option<String>, format = Date)]
    pub day: Option<Date>,
}

/// A heat's Vorlauf counts 2 jury deployments (Konzept: "Vorlauf entspricht
/// 2 Wertungseinsätzen").
pub const HEAT_DEPLOYMENTS: i64 = 2;
/// A final counts 1.
pub const FINAL_DEPLOYMENTS: i64 = 1;
/// 5 judges per task allow dropping the highest and lowest score (Konzept
/// Gemeinsame Wettkämpfe 4.1).
pub const NEEDED_JUDGES: i64 = 5;
pub const NEEDED_DISMOUNT_COUNTERS: i64 = 3;

/// The settings of a jury unit as a heat or final stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSettings {
    pub registrable: bool,
    pub deployments: i64,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

impl UnitSettings {
    /// What a heat or final gets without settings (JURY-2): built into the
    /// binary.
    pub fn default_for(kind: JuryUnitKind) -> Self {
        Self {
            registrable: true,
            deployments: match kind {
                JuryUnitKind::Heat => HEAT_DEPLOYMENTS,
                JuryUnitKind::Final => FINAL_DEPLOYMENTS,
            },
            needed_technical: NEEDED_JUDGES,
            needed_performance: NEEDED_JUDGES,
            needed_dismounts: NEEDED_DISMOUNT_COUNTERS,
        }
    }

    /// The default with what a competition file sets; fails with an English
    /// message on a negative number.
    pub fn resolve(
        kind: JuryUnitKind,
        registrable: Option<bool>,
        deployments: Option<i64>,
        needed_technical: Option<i64>,
        needed_performance: Option<i64>,
        needed_dismounts: Option<i64>,
    ) -> Result<Self, String> {
        let default = Self::default_for(kind);
        let settings = Self {
            registrable: registrable.unwrap_or(default.registrable),
            deployments: deployments.unwrap_or(default.deployments),
            needed_technical: needed_technical.unwrap_or(default.needed_technical),
            needed_performance: needed_performance.unwrap_or(default.needed_performance),
            needed_dismounts: needed_dismounts.unwrap_or(default.needed_dismounts),
        };
        if [
            settings.deployments,
            settings.needed_technical,
            settings.needed_performance,
            settings.needed_dismounts,
        ]
        .iter()
        .any(|n| *n < 0)
        {
            return Err("the jury deployments and needed judges can't be negative".to_string());
        }
        Ok(settings)
    }

    /// The settings as fields of the file: `None` where they are the default.
    pub fn for_file(self, kind: JuryUnitKind) -> [Option<i64>; 5] {
        let default = Self::default_for(kind);
        let differs = |value: i64, default: i64| (value != default).then_some(value);
        [
            (self.registrable != default.registrable).then_some(i64::from(self.registrable)),
            differs(self.deployments, default.deployments),
            differs(self.needed_technical, default.needed_technical),
            differs(self.needed_performance, default.needed_performance),
            differs(self.needed_dismounts, default.needed_dismounts),
        ]
    }
}

/// What the jury lead sets for a jury unit (JURY-2, D2).
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryUnitFields {
    pub kind: JuryUnitKind,
    pub id: JuryUnitId,
    pub registrable: bool,
    pub deployments: i64,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

impl JuryUnitFields {
    /// Checks the numbers. The message is German.
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.deployments,
            self.needed_technical,
            self.needed_performance,
            self.needed_dismounts,
        ]
        .iter()
        .any(|n| *n < 0)
        {
            return Err("Einsätze und benötigte JurorInnen dürfen nicht negativ sein.".to_string());
        }
        Ok(())
    }
}

/// What a judge does in a jury unit.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum JudgeTask {
    /// Technik.
    Technical,
    /// Performance (Präsentation).
    Performance,
    /// Abstiege: counting dismounts, also open to untrained people.
    Dismounts,
}

/// A judge's task in a jury unit, as a club registers it.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
pub struct JudgeRegistration {
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

/// A registration as listed: with whether the jury lead has to move it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ListedRegistration {
    pub unit_kind: JuryUnitKind,
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
    /// Its unit takes no registrations (a skipped Vorlauf, or switched off):
    /// it still counts, but the jury lead should move it (D7).
    pub flagged: bool,
}

/// A registration of a migrated jury category that matched no heat or final
/// (JURY-5), for the jury lead to move to one. It counts for nothing meanwhile.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct UnmappedRegistration {
    pub id: i64,
    /// The name of the old jury category.
    pub old_unit: String,
    pub task: JudgeTask,
}

/// A club's judge with what they registered for.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ClubJudge {
    pub id: JudgeId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: OffsetDateTime,
    pub is_trained: bool,
    pub max_deployments: Option<i64>,
    pub note: Option<String>,
    pub registrations: Vec<ListedRegistration>,
    pub unmapped_registrations: Vec<UnmappedRegistration>,
    /// What the jury units they registered for are worth (each once).
    pub registered_deployments: i64,
    /// What of it counts for the club: see [`counted_deployments`].
    pub counted_deployments: i64,
}

/// What clubs set for a judge.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct JudgeFields {
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: OffsetDateTime,
    pub is_trained: bool,
    #[serde(default)]
    pub max_deployments: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub registrations: Vec<JudgeRegistration>,
}

/// Age in whole years on `day`.
///
/// ```
/// use db_core::jury::age_on;
/// use time::macros::date;
///
/// assert_eq!(age_on(date!(2012 - 06 - 05), date!(2027 - 06 - 05)), 15);
/// assert_eq!(age_on(date!(2012 - 06 - 06), date!(2027 - 06 - 05)), 14);
/// assert_eq!(age_on(date!(2008 - 02 - 29), date!(2027 - 02 - 28)), 18);
/// ```
pub fn age_on(birthday: Date, day: Date) -> i64 {
    let mut age = i64::from(day.year() - birthday.year());
    if (day.month() as u8, day.day()) < (birthday.month() as u8, birthday.day()) {
        age -= 1;
    }
    age
}

/// Names end up in CSV exports, so they must not start like a formula.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(['=', '+', '-', '@'])
}

/// The `heat_id` and `final_id` columns for a unit: exactly one is set.
pub fn unit_columns(kind: JuryUnitKind, id: JuryUnitId) -> (Option<HeatId>, Option<FinalId>) {
    match kind {
        JuryUnitKind::Heat => (Some(HeatId(id.0)), None),
        JuryUnitKind::Final => (None, Some(FinalId(id.0))),
    }
}

/// A judge's task in a jury unit of the Juryplan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryAssignment {
    pub judge_id: JudgeId,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

/// The age rule of the competition's jury (Jury-Konzept): trained judges are
/// at least `min_age` on the first competition day. `None` where the
/// competition has no jury rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgeAgeRule {
    pub min_age: i64,
    pub competition_start: Date,
}

impl JudgeFields {
    /// Trims the texts, removes duplicate registrations and checks the rules
    /// of the Jury-Konzept: trained judges are at least `min_age` on the first
    /// competition day; untrained people only count dismounts. The message is
    /// German.
    pub fn normalize(mut self, rule: Option<JudgeAgeRule>) -> Result<Self, String> {
        people::trim_name(&mut self.firstname);
        people::trim_name(&mut self.lastname);
        if !is_safe_name(&self.firstname) || !is_safe_name(&self.lastname) {
            return Err(
                "Bitte Vor- und Nachnamen angeben (nicht mit =, +, - oder @ beginnend)."
                    .to_string(),
            );
        }
        self.note = self
            .note
            .map(|note| note.trim().to_string())
            .filter(|note| !note.is_empty());
        if self.max_deployments.is_some_and(|max| max < 1) {
            return Err("Die maximalen Einsätze müssen mindestens 1 sein.".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        self.registrations.retain(|r| seen.insert(r.clone()));
        if !self.is_trained
            && self
                .registrations
                .iter()
                .any(|r| r.task != JudgeTask::Dismounts)
        {
            return Err(
                "Nicht geschulte Personen können nur als AbstiegszählerIn gemeldet werden."
                    .to_string(),
            );
        }
        if self.is_trained
            && let Some(rule) = rule
        {
            let age = age_on(self.birthdate.date(), rule.competition_start);
            if age < rule.min_age {
                return Err(format!(
                    "Geschulte JurorInnen müssen am ersten Wettkampftag mindestens {} Jahre alt sein.",
                    rule.min_age
                ));
            }
        }
        Ok(self)
    }
}

/// The message for a registration into a unit that takes none (D6).
pub const NOT_REGISTRABLE_MESSAGE: &str = "Für diesen Vorlauf oder dieses Finale kann niemand mehr gemeldet werden. Bitte einen anderen wählen.";

/// A judge registration a replace dropped, as in the remap file. Filling in
/// `jury_unit` (`heat: <name>` or `final: <name>`) says which new heat's
/// Vorlauf or final the judge belongs to.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RemapRegistration {
    pub club: String,
    pub firstname: String,
    pub lastname: String,
    pub task: JudgeTask,
    pub old_jury_unit: String,
    /// Empty until filled in.
    pub jury_unit: String,
}

/// Why an entry of the remap file wasn't registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemapProblem {
    /// No judge (or more than one) of that name in that club.
    JudgeNotFound,
    /// No jury unit of that name (`heat: <name>` or `final: <name>`).
    JuryUnitNotFound,
    /// The jury unit takes no registrations (D6): its Vorlauf is skipped or
    /// the jury lead switched them off.
    JuryUnitNotRegistrable,
}

/// What applying a remap file did.
#[derive(Debug, Clone, PartialEq)]
pub struct RemapOutcome {
    /// Registrations made again (or already there).
    pub registered: usize,
    /// Entries without a `jury_unit`; nothing was done for them.
    pub unfilled: Vec<RemapRegistration>,
    /// Entries whose judge or jury unit isn't found, or whose jury unit
    /// takes no registrations.
    pub not_found: Vec<(RemapRegistration, RemapProblem)>,
}

/// A jury unit as the database stores it, before the timeplan gives it a day
/// and an order.
#[derive(Debug, Clone)]
pub struct JuryUnitRow {
    pub id: JuryUnitId,
    pub kind: JuryUnitKind,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub registrable: bool,
    pub registrable_setting: bool,
    pub skipped: bool,
    pub deployments: i64,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

/// A category, for the discipline and the timeplan position of its heat.
#[derive(Debug, Clone)]
pub struct JuryUnitCategory {
    pub name: String,
    pub heat_id: Option<HeatId>,
    pub act_type: ActType,
}

/// The final a heat feeds (the lowest id if several).
#[derive(Debug, Clone, Copy)]
pub struct JuryUnitFeed {
    pub heat_id: HeatId,
    pub final_id: FinalId,
}

/// The discipline of a final.
#[derive(Debug, Clone, Copy)]
pub struct JuryUnitFinalType {
    pub id: FinalId,
    pub act_type: ActType,
}

/// Everything [`JuryRepo::jury_unit_rows`] reads, from one consistent view.
#[derive(Debug, Clone, Default)]
pub struct JuryUnitRows {
    pub units: Vec<JuryUnitRow>,
    /// In the order of the timeplan's categories.
    pub categories: Vec<JuryUnitCategory>,
    pub feeds: Vec<JuryUnitFeed>,
    pub final_types: Vec<JuryUnitFinalType>,
}

/// Judges, their registrations and the Juryplan.
#[async_trait::async_trait]
pub trait JuryRepo {
    /// The jury units (every heat's Vorlauf and every final) with the
    /// settings and the data their discipline and day are derived from.
    async fn jury_unit_rows(&self) -> Result<JuryUnitRows, DbError>;

    /// Judges with their registrations: of one club, or of all clubs.
    async fn list_judges(&self, club_id: Option<ClubId>) -> Result<Vec<ClubJudge>, DbError>;

    /// Registers a judge of the club. Normalizes the fields against the
    /// competition's jury rules and refuses (`Rejected`, German message)
    /// duplicate names, unknown jury units and units that take no
    /// registrations. `NotFound` for an unknown club, `Forbidden` unless the
    /// actor may manage it.
    async fn add_judge(
        &self,
        actor: &Actor,
        club_id: ClubId,
        fields: JudgeFields,
    ) -> Result<JudgeId, RegistrationError>;

    /// Changes a judge and replaces their registrations; the club stays the
    /// same. An untrained judge also loses their Technik and Performance
    /// assignments. Same refusals as [`JuryRepo::add_judge`].
    async fn edit_judge(
        &self,
        actor: &Actor,
        judge_id: JudgeId,
        fields: JudgeFields,
    ) -> Result<(), RegistrationError>;

    /// Removes a judge with their registrations and assignments.
    async fn delete_judge(&self, actor: &Actor, judge_id: JudgeId)
    -> Result<(), RegistrationError>;

    /// When the Juryplan was published; `None` while it wasn't (or there is
    /// no competition).
    async fn jury_plan_published_at(&self) -> Result<Option<OffsetDateTime>, DbError>;

    /// The Juryplan's assignments: of one club's judges, or of all.
    async fn list_jury_assignments(
        &self,
        club_id: Option<ClubId>,
    ) -> Result<Vec<JuryAssignment>, DbError>;

    /// Assigns a judge a task in a unit. `NotFound` for an unknown judge or
    /// unit, `Rejected` for an untrained judge with a task other than
    /// dismounts and for a duplicate.
    async fn assign_judge(&self, assignment: &JuryAssignment) -> Result<(), RegistrationError>;

    /// Removes the judge's assignments in a unit (none is fine).
    async fn unassign_judge(&self, judge_id: JudgeId, unit_id: JuryUnitId) -> Result<(), DbError>;

    /// Changes a unit's jury settings. `NotFound` for an unknown unit.
    async fn edit_jury_unit(&self, fields: &JuryUnitFields) -> Result<(), RegistrationError>;

    /// Moves all registrations of a judge from one unit to another; ones the
    /// judge already has there merge. `NotFound` for an unknown target or
    /// nothing to move, `Rejected` for a target that takes no registrations.
    async fn move_judge_registrations(
        &self,
        judge_id: JudgeId,
        from_id: JuryUnitId,
        to_kind: JuryUnitKind,
        to_id: JuryUnitId,
    ) -> Result<(), RegistrationError>;

    /// Registers the judge of an unmapped registration for a unit (or just
    /// drops it with `None`) and removes the unmapped entry.
    async fn resolve_unmapped_registration(
        &self,
        id: i64,
        unit: Option<(JuryUnitKind, JuryUnitId)>,
    ) -> Result<(), RegistrationError>;

    /// Registers the judges of a remap file again, in one unit of work.
    async fn apply_jury_remap(
        &self,
        registrations: Vec<RemapRegistration>,
    ) -> Result<RemapOutcome, DbError>;

    /// Credits a club with jury deployments instead of the registered ones;
    /// `None` uses the registered ones again. `NotFound` for an unknown club.
    async fn set_jury_deployments_override(
        &self,
        club_id: ClubId,
        deployments: Option<i64>,
    ) -> Result<(), RegistrationError>;

    /// Publishes the Juryplan (at `now`) or takes it back. Publishing is
    /// `Rejected` (German message) before the end of the judge registration.
    async fn set_jury_plan_published(
        &self,
        published: bool,
        now: OffsetDateTime,
    ) -> Result<(), RegistrationError>;
}
