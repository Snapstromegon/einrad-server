//! The jury registration (#225): clubs register their judges (JurorInnen)
//! for the heats' Vorlaufs and the finals (jury units), admins check each club's jury
//! obligation and plan the jury. Everything is behind the
//! `jury_registration` feature of the competition (see `Features`).

use crate::ids::{
    CategoryId, ClubId, FinalId, HeatId, JudgeId, JuryAssignmentId, JurySessionId, JuryUnitId,
    RiderId,
};
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use time::{Date, OffsetDateTime};

use crate::{
    act_type::ActType,
    competition::{Competition, Features, JuryRules},
    groups::ensure_club_access,
    http_server::{
        HttpError,
        extractor::auth::Auth,
        routes::query::predict_timeplan::{self, TimeplanEntry},
    },
    jury_sessions::{self, UnitCategory},
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
    /// The Durchgänge of the unit (ADR 0019), in their order; at least one.
    /// An unsplit unit has one holding all its categories.
    pub sessions: Vec<crate::jury_sessions::JurySession>,
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

/// Loads the jury units in the order of the timeplan: by the first block of
/// the heat's categories or of the final; units the timeplan doesn't run
/// last, by name. The day is the planned day of that block.
pub async fn list_units(db: &sqlx::SqlitePool) -> Result<Vec<JuryUnit>, HttpError> {
    let blocks = predicted_blocks(db).await;
    list_units_in(db, &blocks).await
}

/// The blocks of the predicted timeplan; none while it can't be predicted
/// (logged), so the jury pages still work without a timeplan.
async fn predicted_blocks(db: &sqlx::SqlitePool) -> Vec<predict_timeplan::TimeplanItem> {
    match predict_timeplan::predict(db, Audience::OrganizerTeam).await {
        Ok(plan) => plan.items,
        Err(e) => {
            tracing::warn!("No days for the jury units, the timeplan can't be predicted: {e:?}");
            vec![]
        }
    }
}

/// Whether the block runs a category the predicate accepts.
fn runs_category(entry: &TimeplanEntry, wanted: &dyn Fn(CategoryId) -> bool) -> bool {
    matches!(entry, TimeplanEntry::Acts { home_category_id: Some(id), .. } if wanted(*id))
}

/// Whether the block is the final's.
fn runs_final(entry: &TimeplanEntry, wanted: FinalId) -> bool {
    matches!(entry, TimeplanEntry::Acts { final_id: Some(id), .. } if *id == wanted)
}

/// [`list_units`] with the predicted timeplan blocks at hand.
async fn list_units_in(
    db: &sqlx::SqlitePool,
    blocks: &[predict_timeplan::TimeplanItem],
) -> Result<Vec<JuryUnit>, HttpError> {
    let mut conn = db.acquire().await?;
    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!: JuryUnitId",
            kind as "kind!: JuryUnitKind",
            name as "name!",
            code as "code!",
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
    let categories = sqlx::query!(
        r#"SELECT id as "id!: CategoryId", name as "name!", "order", heat_id as "heat_id: HeatId",
            act_type as "act_type!: ActType"
            FROM categories ORDER BY "order", name"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let slots = sqlx::query!(
        r#"SELECT DISTINCT final_id as "final_id!: FinalId", heat_id as "heat_id!: HeatId"
        FROM final_slots WHERE heat_id IS NOT NULL"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let session_rows = jury_sessions::load_session_rows(&mut conn).await?;
    let memberships = jury_sessions::load_memberships(&mut conn).await?;
    let feeds = sqlx::query!(
        r#"SELECT heat_id as "heat_id!: HeatId", MIN(final_id) as "final_id!: FinalId"
        FROM final_slots WHERE heat_id IS NOT NULL GROUP BY heat_id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let final_types = sqlx::query!(
        r#"SELECT id as "id!: FinalId", act_type as "act_type!: ActType" FROM finals"#
    )
    .fetch_all(&mut *conn)
    .await?;
    drop(conn);

    // Where each unit first runs: the position and planned day of its block.
    // Without a timeplan (or without a start time for its first block) there
    // are no days.
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
                JuryUnitKind::Heat => first_block(&|entry| {
                    runs_category(entry, &|category| {
                        categories
                            .iter()
                            .any(|c| c.id == category && c.heat_id == Some(row.id.as_heat()))
                    })
                }),
                JuryUnitKind::Final => first_block(&|entry| runs_final(entry, row.id.as_final())),
            };
            let final_id = match row.kind {
                JuryUnitKind::Heat => feeds
                    .iter()
                    .find(|f| f.heat_id == row.id.as_heat())
                    .map(|f| f.final_id),
                JuryUnitKind::Final => None,
            };
            let own_type =
                |id: FinalId| final_types.iter().find(|f| f.id == id).map(|f| f.act_type);
            let act_type = match row.kind {
                JuryUnitKind::Heat => categories
                    .iter()
                    .find(|c| c.heat_id == Some(row.id.as_heat()))
                    .map(|c| c.act_type)
                    .or_else(|| final_id.and_then(own_type)),
                JuryUnitKind::Final => own_type(row.id.as_final()),
            };
            let unit_categories: Vec<UnitCategory> = categories
                .iter()
                .filter(|c| match row.kind {
                    JuryUnitKind::Heat => c.heat_id == Some(row.id.as_heat()),
                    JuryUnitKind::Final => slots.iter().any(|slot| {
                        slot.final_id == row.id.as_final() && Some(slot.heat_id) == c.heat_id
                    }),
                })
                .map(|c| UnitCategory {
                    id: c.id,
                    name: c.name.clone(),
                    order: c.order,
                })
                .collect();
            // A heat's Durchgänge follow the timeplan; a final's block has no
            // per-category position.
            let position = |category: &UnitCategory| {
                if row.kind == JuryUnitKind::Final {
                    return usize::MAX;
                }
                blocks
                    .iter()
                    .position(|item| {
                        matches!(&item.timeplan_entry, TimeplanEntry::Acts {
                            home_category_id: Some(id), ..
                        } if *id == category.id)
                    })
                    .unwrap_or(usize::MAX)
            };
            let own_sessions: Vec<_> = session_rows
                .iter()
                .filter(|s| s.unit_id == row.id)
                .copied()
                .collect();
            let sessions = jury_sessions::assemble(
                row.id,
                &unit_categories,
                &position,
                &own_sessions,
                &memberships,
                (
                    row.needed_technical,
                    row.needed_performance,
                    row.needed_dismounts,
                ),
            );
            let unit = JuryUnit {
                sessions,
                act_type,
                final_id,
                id: row.id,
                kind: row.kind,
                name: row.name,
                code: row.code,
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
    id: JuryUnitId,
) -> sqlx::Result<Option<bool>> {
    sqlx::query_scalar!(
        r#"SELECT registrable as "registrable!: bool" FROM view_jury_unit WHERE kind = ? AND id = ?"#,
        kind,
        id
    )
    .fetch_optional(conn)
    .await
}

/// A linked judge's rider has an act in a category of a heat (#405).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct RideConflict {
    /// The category the rider rides in.
    pub category: String,
    /// The rider is only a replacement (Ersatzfahrer) there.
    pub replacement: bool,
}

/// One heat a rider rides in: the category, and whether only as replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeatRide {
    pub rider_id: RiderId,
    pub heat_id: HeatId,
    pub category: String,
    pub replacement: bool,
}

/// The heats riders ride in (#405), one row per rider and heat. A rider rides
/// in a heat when they take part (also as replacement) in a not withdrawn act
/// whose category belongs to it; an act without a category rides nowhere. The
/// regular ride is preferred over a replacement ride, then the first category.
/// Each of `club_id`, `rider_id` and `heat_id` narrows the rows when given.
pub(crate) async fn heat_rides(
    conn: &mut sqlx::SqliteConnection,
    club_id: Option<ClubId>,
    rider_id: Option<RiderId>,
    heat_id: Option<HeatId>,
) -> sqlx::Result<Vec<HeatRide>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            p.rider_id as "rider_id!: RiderId",
            c.heat_id as "heat_id!: HeatId",
            c.name as "category!",
            c."order" as "order: i64",
            p.is_replacement as "replacement!: bool"
        FROM act_participants p
        JOIN view_act a ON a.id = p.act_id
        JOIN categories c ON c.id = a.category_id
        JOIN rider r ON r.id = p.rider_id
        WHERE a.withdrawn_at IS NULL
            AND c.heat_id IS NOT NULL
            AND (?1 IS NULL OR r.club_id = ?1)
            AND (?2 IS NULL OR p.rider_id = ?2)
            AND (?3 IS NULL OR c.heat_id = ?3)
        "#,
        club_id,
        rider_id,
        heat_id
    )
    .fetch_all(conn)
    .await?;
    let mut rides: Vec<(bool, Option<i64>, HeatRide)> = rows
        .into_iter()
        .map(|row| {
            (
                row.replacement,
                row.order,
                HeatRide {
                    rider_id: row.rider_id,
                    heat_id: row.heat_id,
                    category: row.category,
                    replacement: row.replacement,
                },
            )
        })
        .collect();
    rides.sort_by(|(a_replacement, a_order, a), (b_replacement, b_order, b)| {
        (a_replacement, a_order.is_none(), a_order, &a.category).cmp(&(
            b_replacement,
            b_order.is_none(),
            b_order,
            &b.category,
        ))
    });
    let mut seen = std::collections::HashSet::new();
    Ok(rides
        .into_iter()
        .map(|(_, _, ride)| ride)
        .filter(|ride| seen.insert((ride.rider_id, ride.heat_id)))
        .collect())
}

/// How `rider_id` rides in the heat `heat_id`, or `None` when they don't.
pub async fn ride_conflict(
    conn: &mut sqlx::SqliteConnection,
    rider_id: RiderId,
    heat_id: HeatId,
) -> sqlx::Result<Option<RideConflict>> {
    Ok(heat_rides(conn, None, Some(rider_id), Some(heat_id))
        .await?
        .into_iter()
        .next()
        .map(|ride| RideConflict {
            category: ride.category,
            replacement: ride.replacement,
        }))
}

/// The refusal for a new registration of a rider who rides in the heat (German).
pub fn ride_conflict_message(name: &str, conflict: &RideConflict) -> String {
    format!(
        "{name} fährt in {}{} und kann deshalb nicht für diesen Vorlauf gemeldet werden.",
        conflict.category,
        if conflict.replacement {
            " als Ersatzfahrer"
        } else {
            ""
        }
    )
}

/// The refusal for a unit that takes no registrations (D6, German message).
pub fn not_registrable() -> HttpError {
    HttpError::ErrorMessages(
        "Für diesen Vorlauf oder dieses Finale kann niemand mehr gemeldet werden. Bitte einen anderen wählen."
            .to_string(),
    )
}

/// Refuses (409, German message) to delete the heat or final `id` while
/// judges are registered or assigned to its jury unit (D4).
pub async fn ensure_unit_unused(
    conn: &mut sqlx::SqliteConnection,
    kind: JuryUnitKind,
    id: JuryUnitId,
) -> Result<(), HttpError> {
    let used = sqlx::query_scalar!(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM judge_registration r JOIN view_jury_unit u ON u.id = r.unit_id
                WHERE u.kind = ?1 AND u.id = ?2)
            + (SELECT COUNT(*) FROM jury_assignments a JOIN view_jury_unit u ON u.id = a.unit_id
                WHERE u.kind = ?1 AND u.id = ?2)
        ) as "count!: i64"
        "#,
        kind,
        id
    )
    .fetch_one(conn)
    .await?;
    if used > 0 {
        return Err(HttpError::Conflict(
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
    /// The rider this judge was created from: their name and birthdate are
    /// the rider's, live. `None` for a judge typed by hand.
    pub rider_id: Option<RiderId>,
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
    /// Name and birthdate are required for a judge typed by hand, and
    /// ignored for a linked judge (the rider's count).
    #[serde(default)]
    pub firstname: Option<String>,
    #[serde(default)]
    pub lastname: Option<String>,
    #[serde(default, with = "time::serde::iso8601::option")]
    pub birthdate: Option<OffsetDateTime>,
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

/// The name and birthdate the jury rules run on: the rider's for a linked
/// judge, the typed ones otherwise.
#[derive(Debug, Clone)]
pub struct ResolvedIdentity {
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
}

fn already_linked(firstname: &str, lastname: &str) -> HttpError {
    HttpError::ErrorMessages(format!(
        "{firstname} {lastname} ist bereits als JurorIn gemeldet."
    ))
}

/// The rider a judge is linked to, or `None` for a judge typed by hand (or an
/// unknown judge).
pub async fn linked_rider(
    conn: &mut sqlx::SqliteConnection,
    judge_id: JudgeId,
) -> Result<Option<RiderId>, HttpError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT rider_id as "rider_id: RiderId" FROM judge WHERE id = ?"#,
        judge_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .flatten())
}

/// The name and birthdate of a judge of `club_id`: the rider's when
/// `rider_id` is given, else the ones typed in `fields`. A rider must belong
/// to the club and must not be linked to another judge than `judge_id` (the
/// one being edited); an unknown rider is a 404. The messages are German.
pub async fn resolve_identity(
    conn: &mut sqlx::SqliteConnection,
    judge_id: Option<JudgeId>,
    club_id: ClubId,
    rider_id: Option<RiderId>,
    fields: &JudgeFields,
) -> Result<ResolvedIdentity, HttpError> {
    let Some(rider_id) = rider_id else {
        let mut firstname = fields.firstname.clone().unwrap_or_default();
        let mut lastname = fields.lastname.clone().unwrap_or_default();
        people::trim_name(&mut firstname);
        people::trim_name(&mut lastname);
        if !is_safe_name(&firstname) || !is_safe_name(&lastname) {
            return Err(HttpError::ErrorMessages(
                "Bitte Vor- und Nachnamen angeben (nicht mit =, +, - oder @ beginnend)."
                    .to_string(),
            ));
        }
        let birthdate = fields.birthdate.ok_or_else(|| {
            HttpError::ErrorMessages("Bitte das Geburtsdatum angeben.".to_string())
        })?;
        return Ok(ResolvedIdentity {
            firstname,
            lastname,
            birthdate,
        });
    };
    let rider = sqlx::query!(
        r#"
        SELECT
            club_id as "club_id!: ClubId",
            firstname as "firstname!",
            lastname as "lastname!",
            birthdate as "birthdate!: OffsetDateTime"
        FROM rider WHERE id = ?
        "#,
        rider_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if rider.club_id != club_id {
        return Err(HttpError::ErrorMessages(
            "Nur FahrerInnen des eigenen Vereins können als JurorIn gemeldet werden.".to_string(),
        ));
    }
    let taken = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM judge WHERE rider_id = ? AND id IS NOT ?"#,
        rider_id,
        judge_id
    )
    .fetch_one(&mut *conn)
    .await?;
    if taken > 0 {
        return Err(already_linked(&rider.firstname, &rider.lastname));
    }
    Ok(ResolvedIdentity {
        firstname: rider.firstname,
        lastname: rider.lastname,
        birthdate: rider.birthdate,
    })
}

impl JudgeFields {
    /// Trims the note, removes duplicate registrations and checks the rules
    /// of the Jury-Konzept: trained judges are at least `min_age` on the first
    /// competition day (by the `identity`'s birthdate); untrained people only
    /// count dismounts. The message is German.
    pub fn normalize(
        mut self,
        competition: Option<&Competition>,
        identity: &ResolvedIdentity,
    ) -> Result<Self, String> {
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
            let age = age_on(identity.birthdate.date(), competition.start_date);
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
    judge_id: JudgeId,
) -> Result<ClubId, HttpError> {
    let club_id = sqlx::query_scalar!(
        r#"SELECT club_id as "club_id!: ClubId" FROM judge WHERE id = ?"#,
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
/// message. `identity` comes from [`resolve_identity`] for the same
/// `rider_id`: a linked judge stores no name or birthdate of their own, their
/// rider's count (the names of a linked judge are only checked when the link
/// is made).
pub async fn save_judge(
    conn: &mut sqlx::SqliteConnection,
    judge_id: JudgeId,
    club_id: ClubId,
    rider_id: Option<RiderId>,
    identity: &ResolvedIdentity,
    fields: &JudgeFields,
) -> Result<(), HttpError> {
    if rider_id.is_none() || linked_rider(conn, judge_id).await? != rider_id {
        people::ensure_name_is_free(
            conn,
            Person::Judge,
            club_id,
            &identity.firstname,
            &identity.lastname,
            Some(judge_id.0),
        )
        .await?;
    }
    let (firstname, lastname, birthdate) = match rider_id {
        Some(_) => (None, None, None),
        None => (
            Some(&identity.firstname),
            Some(&identity.lastname),
            Some(identity.birthdate),
        ),
    };
    let result = sqlx::query!(
        r#"
        INSERT INTO judge (
            id, club_id, rider_id, firstname, lastname, birthdate, is_trained, max_deployments, note
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        -- rider_id is left out on purpose: the link never changes on edit.
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
        rider_id,
        firstname,
        lastname,
        birthdate,
        fields.is_trained,
        fields.max_deployments,
        fields.note,
    )
    .execute(&mut *conn)
    .await;
    if let Err(e) = result {
        return Err(if rider_id.is_some() && people::is_unique_violation(&e) {
            already_linked(&identity.firstname, &identity.lastname)
        } else {
            people::map_duplicate(e, &identity.firstname, &identity.lastname)
        });
    }
    let existing: std::collections::HashSet<(JuryUnitKind, JuryUnitId, JudgeTask)> = sqlx::query!(
        r#"
        SELECT
            CASE WHEN heat_id IS NULL THEN 'final' ELSE 'heat' END as "kind!: JuryUnitKind",
            unit_id as "unit_id!: JuryUnitId",
            task as "task!: JudgeTask"
        FROM judge_registration WHERE judge_id = ?
        "#,
        judge_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| (row.kind, row.unit_id, row.task))
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
            // Only what the judge already has there may stay, per task.
            Some(false)
                if !existing.contains(&(
                    registration.unit_kind,
                    registration.unit_id,
                    registration.task,
                )) =>
            {
                return Err(not_registrable());
            }
            Some(_) => {}
        }
        // A rider can't judge the Vorlauf they ride in (#405). Only new
        // registrations: what the judge already has stays, like for a closed
        // unit. Finals are #408.
        if registration.unit_kind == JuryUnitKind::Heat
            && let Some(rider_id) = rider_id
            && !existing.contains(&(
                registration.unit_kind,
                registration.unit_id,
                registration.task,
            ))
            && let Some(conflict) =
                ride_conflict(conn, rider_id, HeatId(registration.unit_id.0)).await?
        {
            return Err(HttpError::ErrorMessages(ride_conflict_message(
                &format!("{} {}", identity.firstname, identity.lastname),
                &conflict,
            )));
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
pub fn unit_columns(kind: JuryUnitKind, id: JuryUnitId) -> (Option<HeatId>, Option<FinalId>) {
    match kind {
        JuryUnitKind::Heat => (Some(HeatId(id.0)), None),
        JuryUnitKind::Final => (None, Some(FinalId(id.0))),
    }
}

/// Loads judges with their registrations: of one club, or of all clubs.
pub async fn list_judges(
    conn: &mut sqlx::SqliteConnection,
    club_id: Option<ClubId>,
) -> sqlx::Result<Vec<ClubJudge>> {
    let mut registrations: std::collections::HashMap<JudgeId, Vec<ListedRegistration>> =
        std::collections::HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration.judge_id as "judge_id!: JudgeId",
            judge_registration.unit_id as "unit_id!: JuryUnitId",
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
    let mut unmapped: std::collections::HashMap<JudgeId, Vec<UnmappedRegistration>> =
        std::collections::HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            judge_registration_unmapped.id as "id!: i64",
            judge_registration_unmapped.judge_id as "judge_id!: JudgeId",
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
            judge.id as "id!: JudgeId",
            judge.club_id as "club_id!: ClubId",
            clubs.name as club_name,
            judge.rider_id as "rider_id: RiderId",
            judge.firstname as "firstname!: String",
            judge.lastname as "lastname!: String",
            judge.birthdate as "birthdate!: OffsetDateTime",
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
        FROM view_judge judge
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
            rider_id: row.rider_id,
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
    club_id: Option<ClubId>,
) -> sqlx::Result<std::collections::HashMap<ClubId, JuryObligation>> {
    let clubs = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: ClubId",
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
            judge.club_id as "club_id!: ClubId",
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
    let mut registered: std::collections::HashMap<ClubId, i64> = std::collections::HashMap::new();
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

/// A judge's task in a Durchgang (jury session) of a jury unit of the
/// Juryplan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct JuryAssignment {
    pub id: JuryAssignmentId,
    pub judge_id: JudgeId,
    /// The Durchgang; its unit is `unit_id`.
    pub session_id: JurySessionId,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
    /// The seat number within the Durchgang and task (1-based, ADR 0020).
    pub seat: i64,
    /// The seat's name: T, P or A (by task), then the number, e.g. "T3".
    pub seat_label: String,
    /// The heat's or the final's readable name.
    pub unit_name: String,
    /// The Durchgang's category names joined by ", "; empty for an unsplit
    /// unit.
    pub session_label: String,
    /// The planned day of the Durchgang's first block; `None` while it isn't
    /// in the timeplan (or the timeplan can't be predicted).
    #[serde(with = "iso_date::option")]
    #[schema(value_type = Option<String>, format = Date)]
    pub day: Option<Date>,
    /// The title of that block (a block is named "<title> (HH:MM)").
    pub block_title: Option<String>,
    /// The planned start of that block.
    #[serde(with = "time::serde::rfc3339::option")]
    pub starts_at: Option<OffsetDateTime>,
}

/// When the Juryplan was published; `None` while only the jury lead sees it.
pub async fn plan_published_at(
    conn: &mut sqlx::SqliteConnection,
) -> sqlx::Result<Option<OffsetDateTime>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT jury_plan_published_at as "published_at: OffsetDateTime" FROM competition ORDER BY start_date DESC LIMIT 1"#
    )
    .fetch_optional(conn)
    .await?
    .flatten())
}

/// The Juryplan: one entry per assignment (so per seat) of all judges or of a
/// club's, with the Durchgang's label, day, block and time, sorted by time
/// (entries without one last), then unit, Durchgang, task and seat. A
/// Durchgang's block is the first one running its categories (a final's: the
/// final's own), as the timeplan predicts it.
pub async fn plan_assignments(
    db: &sqlx::SqlitePool,
    club: Option<ClubId>,
) -> Result<Vec<JuryAssignment>, HttpError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            jury_assignments.id as "id!: JuryAssignmentId",
            jury_assignments.session_id as "session_id!: JurySessionId",
            jury_assignments.judge_id as "judge_id!: JudgeId",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            jury_assignments.unit_id as "unit_id!: JuryUnitId",
            jury_assignments.task as "task!: JudgeTask",
            jury_assignments.seat as "seat!: i64"
        FROM jury_assignments
        JOIN view_judge judge ON judge.id = jury_assignments.judge_id
        JOIN view_jury_unit ON view_jury_unit.id = jury_assignments.unit_id
        WHERE ?1 IS NULL OR judge.club_id = ?1
        "#,
        club
    )
    .fetch_all(db)
    .await?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let blocks = predicted_blocks(db).await;
    let units = list_units_in(db, &blocks).await?;
    let mut entries: Vec<(usize, JuryAssignment)> = rows
        .into_iter()
        .map(|row| {
            let unit = units.iter().find(|u| u.id == row.unit_id);
            let session = unit.and_then(|u| u.sessions.iter().find(|s| s.id == row.session_id));
            let block = unit.zip(session).and_then(|(unit, session)| {
                blocks.iter().find(|item| match unit.kind {
                    JuryUnitKind::Heat => {
                        runs_category(&item.timeplan_entry, &|category| {
                            session.categories.iter().any(|c| c.id == category)
                        }) && !matches!(
                            item.timeplan_entry,
                            TimeplanEntry::Acts {
                                preliminary_skipped: true,
                                ..
                            }
                        )
                    }
                    JuryUnitKind::Final => runs_final(&item.timeplan_entry, unit.id.as_final()),
                })
            });
            let title = block.and_then(|item| match &item.timeplan_entry {
                TimeplanEntry::Acts { description, .. } => Some(description.clone()),
                _ => None,
            });
            let session_label = match (unit, session) {
                (Some(unit), Some(session)) if unit.sessions.len() > 1 => session
                    .categories
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => String::new(),
            };
            (
                session.map_or(0, |s| s.number as usize),
                JuryAssignment {
                    id: row.id,
                    judge_id: row.judge_id,
                    session_id: row.session_id,
                    unit_kind: row.unit_kind,
                    unit_id: row.unit_id,
                    task: row.task,
                    seat: row.seat,
                    seat_label: jury_sessions::seat_label(row.task, row.seat),
                    unit_name: unit.map(|u| u.name.clone()).unwrap_or_default(),
                    session_label,
                    day: block.map(|item| item.planned_start.date()),
                    block_title: title,
                    starts_at: block.map(|item| item.planned_start),
                },
            )
        })
        .collect();
    entries.sort_by(|(a_number, a), (b_number, b)| {
        let key = |number: &usize, a: &JuryAssignment| {
            (
                a.starts_at.is_none(),
                a.starts_at,
                a.unit_name.clone(),
                *number,
                a.task as u8,
                a.seat,
                a.id,
            )
        };
        key(a_number, a).cmp(&key(b_number, b))
    });
    Ok(entries.into_iter().map(|(_, a)| a).collect())
}
