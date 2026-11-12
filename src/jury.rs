//! The jury registration (#225): clubs register their judges (JurorInnen)
//! for the heats' Vorlaufs and the finals (jury units), admins check each club's jury
//! obligation and plan the jury. Everything is behind the
//! `jury_registration` feature of the competition (see `Features`).

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{
    competition::{Competition, Features, JuryRules},
    groups::ensure_club_access,
    http_server::{
        HttpError,
        extractor::auth::Auth,
        routes::query::predict_timeplan::{self, TimeplanEntry},
    },
    people::{self, Person},
    reloadable_sqlite::ReloadableSqlite,
    timeplan::Audience,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// Guard for every jury endpoint: they don't exist (404) while the feature
/// is off. Read per request, so turning it on or off applies immediately.
#[derive(Debug)]
pub struct JuryEnabled;

impl<S: Send + Sync> FromRequestParts<S> for JuryEnabled {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(db) = parts.extensions.get::<ReloadableSqlite>() else {
            return Err(HttpError::NotFound);
        };
        let db = db.get().await.clone();
        if Features::current(&db).await?.jury_registration {
            Ok(JuryEnabled)
        } else {
            Err(HttpError::NotFound)
        }
    }
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
    pub id: Uuid,
    pub kind: JuryUnitKind,
    /// The heat's or the final's name; renaming it keeps the registrations.
    pub name: String,
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
    pub id: Uuid,
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

/// Loads the jury units in the order of the timeplan: by the first block of
/// the heat's categories or of the final; units the timeplan doesn't run
/// last, by name. The day is the planned day of that block.
pub async fn list_units(db: &sqlx::SqlitePool) -> Result<Vec<JuryUnit>, HttpError> {
    let mut conn = db.acquire().await?;
    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!: Uuid",
            kind as "kind!: JuryUnitKind",
            name as "name!",
            description,
            registrable as "registrable!: bool",
            registrable_setting as "registrable_setting!: bool",
            skipped as "skipped!: bool",
            deployments as "deployments!: i64",
            needed_technical as "needed_technical!: i64",
            needed_performance as "needed_performance!: i64",
            needed_dismounts as "needed_dismounts!: i64"
        FROM view_jury_unit
        "#
    )
    .fetch_all(&mut *conn)
    .await?;
    let categories = sqlx::query!(r#"SELECT name as "name!", heat FROM categories"#)
        .fetch_all(&mut *conn)
        .await?;
    drop(conn);

    // Where each unit first runs: the position and planned day of its block.
    // Without a timeplan (or without a start time for its first block) there
    // are no days.
    let blocks = match predict_timeplan::predict(db, Audience::OrganizerTeam).await {
        Ok(plan) => plan.items,
        Err(_) => vec![],
    };
    let first_block = |matches: &dyn Fn(&TimeplanEntry) -> bool| {
        blocks
            .iter()
            .enumerate()
            .find(|(_, item)| matches(&item.timeplan_entry))
            .map(|(position, item)| (position, item.planned_start.date()))
    };
    let mut units: Vec<(Option<(usize, Date)>, JuryUnit)> = rows
        .into_iter()
        .map(|row| {
            let block = match row.kind {
                JuryUnitKind::Heat => first_block(&|entry| match entry {
                    TimeplanEntry::Acts {
                        home_category: Some(category),
                        ..
                    } => categories
                        .iter()
                        .any(|c| &c.name == category && c.heat.as_deref() == Some(&row.name)),
                    _ => false,
                }),
                JuryUnitKind::Final => first_block(&|entry| match entry {
                    TimeplanEntry::Acts {
                        final_name: Some(name),
                        ..
                    } => *name == row.name,
                    _ => false,
                }),
            };
            let unit = JuryUnit {
                id: row.id,
                kind: row.kind,
                name: row.name,
                description: row.description.filter(|d| !d.trim().is_empty()),
                registrable: row.registrable,
                registrable_setting: row.registrable_setting,
                skipped: row.skipped,
                deployments: row.deployments,
                needed_technical: row.needed_technical,
                needed_performance: row.needed_performance,
                needed_dismounts: row.needed_dismounts,
                day: block.map(|(_, day)| day),
            };
            (block, unit)
        })
        .collect();
    units.sort_by(|(a_block, a), (b_block, b)| {
        let key = |block: &Option<(usize, Date)>, unit: &JuryUnit| {
            (
                block.map_or(usize::MAX, |(position, _)| position),
                unit.kind == JuryUnitKind::Final,
                unit.name.clone(),
            )
        };
        key(a_block, a).cmp(&key(b_block, b))
    });
    Ok(units.into_iter().map(|(_, unit)| unit).collect())
}

/// The jury unit a heat or final has, by its id, or `None`: whether judges
/// can register for it now.
pub async fn unit_is_registrable(
    conn: &mut sqlx::SqliteConnection,
    kind: JuryUnitKind,
    id: Uuid,
) -> sqlx::Result<Option<bool>> {
    sqlx::query_scalar!(
        r#"SELECT registrable as "registrable!: bool" FROM view_jury_unit WHERE kind = ? AND id = ?"#,
        kind,
        id
    )
    .fetch_optional(conn)
    .await
}

/// Refuses (German message) to delete the heat or final `name` while judges
/// are registered or assigned to its jury unit (D4).
pub async fn ensure_unit_unused(
    conn: &mut sqlx::SqliteConnection,
    kind: JuryUnitKind,
    name: &str,
) -> Result<(), HttpError> {
    let used = sqlx::query_scalar!(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM judge_registration r JOIN view_jury_unit u ON u.id = r.unit_id
                WHERE u.kind = ?1 AND u.name = ?2)
            + (SELECT COUNT(*) FROM jury_assignments a JOIN view_jury_unit u ON u.id = a.unit_id
                WHERE u.kind = ?1 AND u.name = ?2)
        ) as "count!: i64"
        "#,
        kind,
        name
    )
    .fetch_one(conn)
    .await?;
    if used > 0 {
        return Err(HttpError::ErrorMessages(
            "Für die Jury dieser Vorlaufklasse oder dieses Finales sind JurorInnen gemeldet oder eingeteilt. Erst das ändern, dann löschen."
                .to_string(),
        ));
    }
    Ok(())
}

pub(crate) use crate::people::is_unique_violation;

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
    pub unit_id: Uuid,
    pub task: JudgeTask,
}

/// A registration as listed: with whether the jury lead has to move it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ListedRegistration {
    pub unit_kind: JuryUnitKind,
    pub unit_id: Uuid,
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
    pub id: Uuid,
    pub club_id: Uuid,
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
/// use einrad_server::jury::age_on;
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

impl JudgeFields {
    /// Trims the texts, removes duplicate registrations and checks the rules
    /// of the Jury-Konzept: trained judges are at least `min_age` on the first
    /// competition day; untrained people only count dismounts. The message is
    /// German.
    pub fn normalize(mut self, competition: Option<&Competition>) -> Result<Self, String> {
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
            && let Some(competition) = competition
            && let Some(rules) = &competition.jury
        {
            let age = age_on(self.birthdate.date(), competition.start_date);
            if age < rules.min_age {
                return Err(format!(
                    "Geschulte JurorInnen müssen am ersten Wettkampftag mindestens {} Jahre alt sein.",
                    rules.min_age
                ));
            }
        }
        Ok(self)
    }
}

/// Loads the club of a judge and checks that the user may manage it (see
/// [`ensure_club_access`]). Fails with 404 for an unknown judge.
pub async fn ensure_judge_access(
    conn: &mut sqlx::SqliteConnection,
    auth: &Auth,
    judge_id: Uuid,
) -> Result<Uuid, HttpError> {
    let club_id = sqlx::query_scalar!(
        r#"SELECT club_id as "club_id!: Uuid" FROM judge WHERE id = ?"#,
        judge_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    ensure_club_access(conn, auth, club_id).await?;
    Ok(club_id)
}

/// Writes a judge's fields and replaces their registrations; an untrained
/// judge also loses their Technik and Performance assignments. Unknown jury
/// units, new registrations for a unit that takes none (D6; the judge's
/// existing ones may stay) and duplicate names are refused with a German
/// message.
pub async fn save_judge(
    conn: &mut sqlx::SqliteConnection,
    judge_id: Uuid,
    club_id: Uuid,
    fields: &JudgeFields,
) -> Result<(), HttpError> {
    people::ensure_name_is_free(
        conn,
        Person::Judge,
        club_id,
        &fields.firstname,
        &fields.lastname,
        Some(judge_id),
    )
    .await?;
    let result = sqlx::query!(
        r#"
        INSERT INTO judge (
            id, club_id, firstname, lastname, birthdate, is_trained, max_deployments, note
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (id) DO UPDATE SET
            firstname = excluded.firstname,
            lastname = excluded.lastname,
            birthdate = excluded.birthdate,
            is_trained = excluded.is_trained,
            max_deployments = excluded.max_deployments,
            note = excluded.note
        "#,
        judge_id,
        club_id,
        fields.firstname,
        fields.lastname,
        fields.birthdate,
        fields.is_trained,
        fields.max_deployments,
        fields.note,
    )
    .execute(&mut *conn)
    .await;
    match result {
        Ok(_) => {}
        Err(e) => {
            return Err(people::map_duplicate(
                e,
                &fields.firstname,
                &fields.lastname,
            ));
        }
    }
    let existing: std::collections::HashSet<(JuryUnitKind, Uuid)> = sqlx::query!(
        r#"
        SELECT
            CASE WHEN heat_id IS NULL THEN 'final' ELSE 'heat' END as "kind!: JuryUnitKind",
            unit_id as "unit_id!: Uuid"
        FROM judge_registration WHERE judge_id = ?
        "#,
        judge_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| (row.kind, row.unit_id))
    .collect();
    sqlx::query!(
        "DELETE FROM judge_registration WHERE judge_id = ?",
        judge_id
    )
    .execute(&mut *conn)
    .await?;
    if !fields.is_trained {
        // Untrained people only count dismounts, also in the Juryplan.
        sqlx::query!(
            "DELETE FROM jury_assignments WHERE judge_id = ? AND task != 'dismounts'",
            judge_id
        )
        .execute(&mut *conn)
        .await?;
    }
    for registration in &fields.registrations {
        let registrable =
            unit_is_registrable(conn, registration.unit_kind, registration.unit_id).await?;
        match registrable {
            None => {
                return Err(HttpError::ErrorMessages(
                    "Eine gewählte Vorlaufklasse oder ein gewähltes Finale gibt es nicht mehr. Bitte neu laden."
                        .to_string(),
                ));
            }
            Some(false) if !existing.contains(&(registration.unit_kind, registration.unit_id)) => {
                return Err(HttpError::ErrorMessages(
                    "Für eine gewählte Vorlaufklasse oder ein gewähltes Finale kann niemand mehr gemeldet werden. Bitte neu laden."
                        .to_string(),
                ));
            }
            Some(_) => {}
        }
        let (heat_id, final_id) = unit_columns(registration.unit_kind, registration.unit_id);
        sqlx::query!(
            "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES (?, ?, ?, ?)",
            judge_id,
            heat_id,
            final_id,
            registration.task,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// The `heat_id` and `final_id` columns for a unit: exactly one is set.
pub fn unit_columns(kind: JuryUnitKind, id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
    match kind {
        JuryUnitKind::Heat => (Some(id), None),
        JuryUnitKind::Final => (None, Some(id)),
    }
}

/// Loads judges with their registrations: of one club, or of all clubs.
pub async fn list_judges(
    conn: &mut sqlx::SqliteConnection,
    club_id: Option<Uuid>,
) -> sqlx::Result<Vec<ClubJudge>> {
    let mut registrations: std::collections::HashMap<Uuid, Vec<ListedRegistration>> =
        std::collections::HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration.judge_id as "judge_id!: Uuid",
            judge_registration.unit_id as "unit_id!: Uuid",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            judge_registration.task as "task!: JudgeTask",
            NOT view_jury_unit.registrable as "flagged!: bool"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN view_jury_unit ON view_jury_unit.id = judge_registration.unit_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY view_jury_unit.kind DESC, view_jury_unit.name, judge_registration.task
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        registrations
            .entry(row.judge_id)
            .or_default()
            .push(ListedRegistration {
                unit_kind: row.unit_kind,
                unit_id: row.unit_id,
                task: row.task,
                flagged: row.flagged,
            });
    }
    let mut unmapped: std::collections::HashMap<Uuid, Vec<UnmappedRegistration>> =
        std::collections::HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration_unmapped.id as "id!: i64",
            judge_registration_unmapped.judge_id as "judge_id!: Uuid",
            judge_registration_unmapped.old_unit as "old_unit!",
            judge_registration_unmapped.task as "task!: JudgeTask"
        FROM judge_registration_unmapped
        JOIN judge ON judge.id = judge_registration_unmapped.judge_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY judge_registration_unmapped.id
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        unmapped
            .entry(row.judge_id)
            .or_default()
            .push(UnmappedRegistration {
                id: row.id,
                old_unit: row.old_unit,
                task: row.task,
            });
    }
    let judges = sqlx::query!(
        r#"
        SELECT
            judge.id as "id!: Uuid",
            judge.club_id as "club_id!: Uuid",
            clubs.name as club_name,
            judge.firstname,
            judge.lastname,
            judge.birthdate as "birthdate: OffsetDateTime",
            judge.is_trained,
            judge.max_deployments,
            judge.note,
            (
                SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                WHERE c.id IN (
                    SELECT unit_id FROM judge_registration
                    WHERE judge_id = judge.id
                )
            ) as "registered_deployments!: i64",
            (
                SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                WHERE c.id IN (
                    SELECT unit_id FROM judge_registration
                    WHERE judge_id = judge.id AND task != 'dismounts'
                )
            ) as "countable_deployments!: i64"
        FROM judge
        JOIN clubs ON clubs.id = judge.club_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY clubs.name, judge.lastname, judge.firstname
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(judges
        .into_iter()
        .map(|row| ClubJudge {
            registrations: registrations.remove(&row.id).unwrap_or_default(),
            unmapped_registrations: unmapped.remove(&row.id).unwrap_or_default(),
            id: row.id,
            club_id: row.club_id,
            club_name: row.club_name,
            firstname: row.firstname,
            lastname: row.lastname,
            birthdate: row.birthdate,
            is_trained: row.is_trained,
            max_deployments: row.max_deployments,
            note: row.note,
            registered_deployments: row.registered_deployments,
            counted_deployments: counted_deployments(
                row.is_trained,
                row.countable_deployments,
                row.max_deployments,
            ),
        })
        .collect())
}

/// The deployments a judge counts for their club: jury units with a
/// technical or performance registration, at most their maximum; untrained
/// people count none.
///
/// ```
/// use einrad_server::jury::counted_deployments;
///
/// assert_eq!(counted_deployments(true, 6, None), 6);
/// assert_eq!(counted_deployments(true, 6, Some(2)), 2);
/// assert_eq!(counted_deployments(false, 6, None), 0);
/// ```
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
/// use einrad_server::jury::required_deployments;
///
/// assert_eq!(required_deployments(0, 4), 0);
/// assert_eq!(required_deployments(1, 4), 4);
/// assert_eq!(required_deployments(2, 4), 4);
/// assert_eq!(required_deployments(3, 4), 8);
/// assert_eq!(required_deployments(32, 4), 64);
/// ```
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

/// The jury obligation of every club, or only of `club_id`.
///
/// An act counts when it starts: it has a category, it's a
/// group with regular riders (also an incomplete one), or it was withdrawn
/// without the fee being waived. Fee waivers of starting acts, riders and
/// clubs don't lower it. It counts for its owner club: the
/// group's creator, or the club of a single's or pair's riders. A trained
/// judge counts the deployments of every jury unit they registered for
/// as a technical or performance judge (once per unit), at most
/// `max_deployments`. Dismount-only registrations do not count.
pub async fn obligations(
    conn: &mut sqlx::SqliteConnection,
    rules: &JuryRules,
    club_id: Option<Uuid>,
) -> sqlx::Result<std::collections::HashMap<Uuid, JuryObligation>> {
    let clubs = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: Uuid",
            clubs.jury_deployments_override,
            (
                SELECT COUNT(*) FROM view_act AS a
                WHERE NOT (a.withdrawn_at IS NOT NULL AND a.fee_waiver IS NOT NULL)
                    AND (
                        a.category IS NOT NULL
                        OR a.withdrawn_at IS NOT NULL
                        -- Incomplete groups pay start fees, too.
                        OR (a.act_type IN ('group_small', 'group_large') AND a.regular_count > 0)
                    )
                    AND COALESCE(
                        a.owner_club_id,
                        (
                            -- A pair across clubs (only admins link those)
                            -- counts for one of them.
                            SELECT MIN(s.club_id) FROM act_participants p
                            JOIN rider s ON s.id = p.rider_id
                            WHERE p.act_id = a.id AND NOT p.is_replacement
                        )
                    ) = clubs.id
            ) as "acts!: i64"
        FROM clubs
        WHERE ?1 IS NULL OR clubs.id = ?1
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let judges = sqlx::query!(
        r#"
        SELECT
            judge.club_id as "club_id!: Uuid",
            judge.max_deployments,
            (
                SELECT COALESCE(SUM(c.deployments), 0) FROM view_jury_unit c
                WHERE c.id IN (
                    SELECT unit_id FROM judge_registration
                    WHERE judge_id = judge.id AND task != 'dismounts'
                )
            ) as "deployments!: i64"
        FROM judge
        WHERE judge.is_trained AND (?1 IS NULL OR judge.club_id = ?1)
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut registered: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
    for judge in judges {
        let deployments = counted_deployments(true, judge.deployments, judge.max_deployments);
        *registered.entry(judge.club_id).or_default() += deployments;
    }
    Ok(clubs
        .into_iter()
        .map(|club| {
            let required = required_deployments(club.acts, rules.deployments_per_two_acts);
            let registered = registered.get(&club.id).copied().unwrap_or(0);
            let credited = club.jury_deployments_override.unwrap_or(registered);
            let missing = (required - credited).max(0);
            let obligation = JuryObligation {
                acts: club.acts,
                required,
                registered,
                credited_override: club.jury_deployments_override,
                credited,
                missing,
                fee: missing as f64 * rules.missing_deployment_fee,
            };
            (club.id, obligation)
        })
        .collect())
}

/// A judge's task in a jury unit of the Juryplan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryAssignment {
    pub judge_id: Uuid,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: Uuid,
    pub task: JudgeTask,
}
