//! The jury registration (#225): clubs register their judges (JurorInnen)
//! for admin-defined jury categories, admins check each club's jury
//! obligation and plan the jury. Everything is behind the
//! `jury_registration` feature of the competition (see `Features`).

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{
    competition::{Competition, Features, JuryRules},
    groups::ensure_club_access,
    http_server::{HttpError, HttpServerOptions, extractor::auth::Auth},
    people::{self, Person},
    reloadable_sqlite::ReloadableSqlite,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// Guard for every jury endpoint: they don't exist (404) while the feature
/// is off. Read per request, so turning it on or off applies immediately.
#[derive(Debug)]
pub struct JuryEnabled;

impl<S: Send + Sync> FromRequestParts<S> for JuryEnabled {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let (Some(options), Some(db)) = (
            parts.extensions.get::<Arc<HttpServerOptions>>(),
            parts.extensions.get::<ReloadableSqlite>(),
        ) else {
            return Err(HttpError::NotFound);
        };
        let db = db.get().await.clone();
        if Features::current(&db, &options.features)
            .await?
            .jury_registration
        {
            Ok(JuryEnabled)
        } else {
            Err(HttpError::NotFound)
        }
    }
}

/// What judges register for and get assigned to, e.g. one heat block.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct JuryCategory {
    pub id: Uuid,
    pub name: String,
    /// Jury deployments (Einsätze) it counts for: a heat 2, a final 1.
    pub deployments: i64,
    /// The day it runs, if known.
    #[serde(with = "iso_date::option")]
    #[schema(value_type = Option<String>, format = Date)]
    pub day: Option<Date>,
    pub sort_order: i64,
    /// Judges it needs per task.
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

/// What admins set for a jury category.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryCategoryFields {
    pub name: String,
    pub deployments: i64,
    #[serde(default, deserialize_with = "optional_date")]
    #[schema(value_type = Option<String>, format = Date)]
    pub day: Option<Date>,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

/// An optional "YYYY-MM-DD" date. Unlike `iso_date::option`, this also works
/// inside `#[serde(flatten)]`, which hands `null` over as a unit value.
fn optional_date<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Date>, D::Error> {
    use serde::{Deserialize, de::Error};
    Option::<String>::deserialize(deserializer)?
        .map(|day| {
            Date::parse(
                &day,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .map_err(D::Error::custom)
        })
        .transpose()
}

impl JuryCategoryFields {
    /// Trims the name and checks the numbers and that the day is within the
    /// event. The message is German.
    pub fn normalize(mut self, competition: Option<&Competition>) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err("Die Jury-Kategorie braucht einen Namen.".to_string());
        }
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
        if let (Some(day), Some(competition)) = (self.day, competition)
            && (day < competition.start_date || day > competition.end_date)
        {
            return Err(format!(
                "Der Tag muss zwischen {} und {} liegen.",
                competition.start_date, competition.end_date
            ));
        }
        Ok(self)
    }
}

/// Loads the jury categories in their order.
pub async fn list_categories(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Vec<JuryCategory>> {
    sqlx::query_as!(
        JuryCategory,
        r#"
        SELECT
            id as "id!: Uuid",
            name,
            deployments,
            day as "day: Date",
            sort_order,
            needed_technical,
            needed_performance,
            needed_dismounts
        FROM jury_categories
        ORDER BY sort_order, name
        "#
    )
    .fetch_all(db)
    .await
}

/// Inserts a jury category at the end of the order and returns its id. A
/// duplicate name is refused with a German message.
pub async fn insert_category(
    conn: &mut sqlx::SqliteConnection,
    fields: &JuryCategoryFields,
) -> Result<Uuid, HttpError> {
    let id = Uuid::new_v4();
    let result = sqlx::query!(
        r#"
        INSERT INTO jury_categories (
            id, name, deployments, day, sort_order,
            needed_technical, needed_performance, needed_dismounts
        ) VALUES (
            ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM jury_categories),
            ?, ?, ?
        )
        "#,
        id,
        fields.name,
        fields.deployments,
        fields.day,
        fields.needed_technical,
        fields.needed_performance,
        fields.needed_dismounts,
    )
    .execute(&mut *conn)
    .await;
    match result {
        Ok(_) => Ok(id),
        Err(e) if is_unique_violation(&e) => Err(duplicate_name(&fields.name)),
        Err(e) => Err(e.into()),
    }
}

pub(crate) use crate::people::is_unique_violation;

pub(crate) fn duplicate_name(name: &str) -> HttpError {
    HttpError::ErrorMessages(format!("Die Jury-Kategorie „{name}“ gibt es schon."))
}

/// What a judge does in a jury category.
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

/// A judge's task in a jury category.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
pub struct JudgeRegistration {
    pub jury_category_id: Uuid,
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
    pub registrations: Vec<JudgeRegistration>,
    /// What the jury categories they registered for are worth (each once).
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
/// use nrw_freestyle_cup_registration::jury::age_on;
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
/// categories and duplicate names are refused with a German message.
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
        let known = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM jury_categories WHERE id = ?"#,
            registration.jury_category_id
        )
        .fetch_one(&mut *conn)
        .await?;
        if known == 0 {
            return Err(HttpError::ErrorMessages(
                "Eine gewählte Jury-Kategorie gibt es nicht mehr. Bitte neu laden.".to_string(),
            ));
        }
        sqlx::query!(
            "INSERT INTO judge_registration (judge_id, jury_category_id, task) VALUES (?, ?, ?)",
            judge_id,
            registration.jury_category_id,
            registration.task,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Loads judges with their registrations: of one club, or of all clubs.
pub async fn list_judges(
    conn: &mut sqlx::SqliteConnection,
    club_id: Option<Uuid>,
) -> sqlx::Result<Vec<ClubJudge>> {
    let mut registrations: std::collections::HashMap<Uuid, Vec<JudgeRegistration>> =
        std::collections::HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration.judge_id as "judge_id!: Uuid",
            judge_registration.jury_category_id as "jury_category_id!: Uuid",
            judge_registration.task as "task!: JudgeTask"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN jury_categories ON jury_categories.id = judge_registration.jury_category_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        ORDER BY jury_categories.sort_order, judge_registration.task
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?
    {
        registrations
            .entry(row.judge_id)
            .or_default()
            .push(JudgeRegistration {
                jury_category_id: row.jury_category_id,
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
                SELECT COALESCE(SUM(c.deployments), 0) FROM jury_categories c
                WHERE c.id IN (
                    SELECT jury_category_id FROM judge_registration
                    WHERE judge_id = judge.id
                )
            ) as "registered_deployments!: i64"
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
                row.registered_deployments,
                row.max_deployments,
            ),
        })
        .collect())
}

/// The deployments a judge counts for their club: what their jury
/// categories are worth, at most their maximum; untrained people count
/// none.
///
/// ```
/// use nrw_freestyle_cup_registration::jury::counted_deployments;
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
/// use nrw_freestyle_cup_registration::jury::required_deployments;
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
    /// What the club's trained judges registered for.
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
/// judge counts the deployments of every jury category they registered for
/// (once per category, whatever the tasks), at most `max_deployments`.
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
                SELECT COALESCE(SUM(c.deployments), 0) FROM jury_categories c
                WHERE c.id IN (
                    SELECT jury_category_id FROM judge_registration
                    WHERE judge_id = judge.id
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

/// A judge's task in a jury category of the Juryplan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryAssignment {
    pub judge_id: Uuid,
    pub jury_category_id: Uuid,
    pub task: JudgeTask,
}
