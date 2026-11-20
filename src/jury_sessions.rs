//! Jury sessions (Jurydurchgänge, ADR 0019): the set of a jury unit's
//! categories that one jury judges. A unit has one by default; only the jury
//! lead splits it. Registration stays per unit.

use std::collections::{HashMap, HashSet};

use axum::http::StatusCode;
use sqlx::SqliteConnection;

use crate::{
    config::JurySessionConfig,
    http_server::HttpError,
    ids::{CategoryId, JudgeId, JuryAssignmentId, JurySessionId, JuryUnitId},
    jury::{self, JudgeTask, JuryAssignment, JuryUnitKind},
};

/// A category of a jury unit: a heat's own, or one of the heats feeding a
/// final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitCategory {
    pub id: CategoryId,
    pub name: String,
    pub order: Option<i64>,
}

/// Inserts the session every unit starts with. It inherits the unit's needed
/// counts and holds all its categories without any category rows (see
/// [`session_of_category`]).
pub async fn create_default_session(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    unit: JuryUnitId,
) -> sqlx::Result<JurySessionId> {
    let id = JurySessionId::new();
    let (heat_id, final_id) = jury::unit_columns(kind, unit);
    sqlx::query!(
        "INSERT INTO jury_sessions (id, heat_id, final_id) VALUES (?, ?, ?)",
        id,
        heat_id,
        final_id,
    )
    .execute(conn)
    .await?;
    Ok(id)
}

/// The oldest session of the unit (UUIDv7 ids are time-ordered).
pub async fn first_session(
    conn: &mut SqliteConnection,
    unit: JuryUnitId,
) -> sqlx::Result<Option<JurySessionId>> {
    sqlx::query_scalar!(
        r#"SELECT id as "id!: JurySessionId" FROM jury_sessions WHERE unit_id = ? ORDER BY id LIMIT 1"#,
        unit
    )
    .fetch_optional(conn)
    .await
}

/// The categories a unit's sessions divide: a heat's own, or those of the
/// heats that feed a final.
pub async fn unit_categories(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    unit: JuryUnitId,
) -> sqlx::Result<Vec<UnitCategory>> {
    let rows = match kind {
        JuryUnitKind::Heat => sqlx::query!(
            r#"
                SELECT id as "id!: CategoryId", name, "order"
                FROM categories WHERE heat_id = ?
                "#,
            unit
        )
        .fetch_all(conn)
        .await?
        .into_iter()
        .map(|r| UnitCategory {
            id: r.id,
            name: r.name,
            order: r.order,
        })
        .collect(),
        JuryUnitKind::Final => sqlx::query!(
            r#"
                SELECT DISTINCT c.id as "id!: CategoryId", c.name, c."order"
                FROM final_slots fs
                JOIN categories c ON c.heat_id = fs.heat_id
                WHERE fs.final_id = ?
                "#,
            unit
        )
        .fetch_all(conn)
        .await?
        .into_iter()
        .map(|r| UnitCategory {
            id: r.id,
            name: r.name,
            order: r.order,
        })
        .collect(),
    };
    Ok(rows)
}

/// The session each category of the unit is in. Stored rows of categories
/// that left the unit are stale and ignored. A category without a row (it
/// joined after the split) is in the first session: the orphan rule.
pub async fn session_of_category(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    unit: JuryUnitId,
) -> sqlx::Result<HashMap<CategoryId, JurySessionId>> {
    let categories = unit_categories(conn, kind, unit).await?;
    let stored: HashMap<CategoryId, JurySessionId> = sqlx::query!(
        r#"
        SELECT category_id as "category_id!: CategoryId", session_id as "session_id!: JurySessionId"
        FROM jury_session_categories WHERE unit_id = ?
        "#,
        unit
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| (r.category_id, r.session_id))
    .collect();
    let first = first_session(conn, unit).await?;
    Ok(categories
        .into_iter()
        .filter_map(|c| {
            stored
                .get(&c.id)
                .copied()
                .or(first)
                .map(|session| (c.id, session))
        })
        .collect())
}

/// A category of a session.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct SessionCategory {
    pub id: CategoryId,
    pub name: String,
}

/// A jury session (Durchgang) of a jury unit.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct JurySession {
    pub id: JurySessionId,
    /// 1-based, by the categories' timeplan position (a final's by category
    /// order, then name); derived, never stored.
    pub number: i64,
    pub categories: Vec<SessionCategory>,
    /// Effective needed judges per task: the override, else the unit's.
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
    /// The overrides (`None` = the unit's).
    pub override_technical: Option<i64>,
    pub override_performance: Option<i64>,
    pub override_dismounts: Option<i64>,
}

/// A stored session of a unit with its overrides.
#[derive(Debug, Clone, Copy)]
pub struct SessionRow {
    pub id: JurySessionId,
    pub unit_id: JuryUnitId,
    pub technical: Option<i64>,
    pub performance: Option<i64>,
    pub dismounts: Option<i64>,
}

/// Every stored session with its overrides.
pub async fn load_session_rows(conn: &mut SqliteConnection) -> sqlx::Result<Vec<SessionRow>> {
    Ok(sqlx::query!(
        r#"
        SELECT id as "id!: JurySessionId", unit_id as "unit_id!: JuryUnitId",
            needed_technical, needed_performance, needed_dismounts
        FROM jury_sessions
        "#
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|r| SessionRow {
        id: r.id,
        unit_id: r.unit_id,
        technical: r.needed_technical,
        performance: r.needed_performance,
        dismounts: r.needed_dismounts,
    })
    .collect())
}

/// Every stored (unit, category) membership: (unit, category, session).
pub async fn load_memberships(
    conn: &mut SqliteConnection,
) -> sqlx::Result<HashMap<(JuryUnitId, CategoryId), JurySessionId>> {
    Ok(sqlx::query!(
        r#"
        SELECT unit_id as "unit_id!: JuryUnitId", category_id as "category_id!: CategoryId",
            session_id as "session_id!: JurySessionId"
        FROM jury_session_categories
        "#
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|r| ((r.unit_id, r.category_id), r.session_id))
    .collect())
}

/// Builds a unit's sessions, numbered. `rows` are the unit's stored sessions
/// (never empty for a unit that exists), `unit_needed` its counts
/// (technical, performance, dismounts), `position` a category's first block
/// in the timeplan. The orphan rule puts a category without a membership
/// into the first session.
///
/// A session's key is the smallest position of its categories, then the
/// smallest category `order`, then the name; the session id breaks ties.
pub fn assemble(
    unit: JuryUnitId,
    categories: &[UnitCategory],
    position: &dyn Fn(&UnitCategory) -> usize,
    rows: &[SessionRow],
    memberships: &HashMap<(JuryUnitId, CategoryId), JurySessionId>,
    unit_needed: (i64, i64, i64),
) -> Vec<JurySession> {
    let Some(first) = rows.iter().map(|r| r.id).min() else {
        return vec![];
    };
    let known: HashSet<JurySessionId> = rows.iter().map(|r| r.id).collect();
    let mut keyed: Vec<((usize, i64, String, JurySessionId), JurySession)> = rows
        .iter()
        .map(|row| {
            let mut members: Vec<&UnitCategory> = categories
                .iter()
                .filter(|c| {
                    memberships
                        .get(&(unit, c.id))
                        .copied()
                        .filter(|session| known.contains(session))
                        .unwrap_or(first)
                        == row.id
                })
                .collect();
            members.sort_by(|a, b| {
                (a.order.unwrap_or(i64::MAX), &a.name).cmp(&(b.order.unwrap_or(i64::MAX), &b.name))
            });
            let key = (
                members
                    .iter()
                    .map(|c| position(c))
                    .min()
                    .unwrap_or(usize::MAX),
                members
                    .iter()
                    .map(|c| c.order.unwrap_or(i64::MAX))
                    .min()
                    .unwrap_or(i64::MAX),
                members.first().map(|c| c.name.clone()).unwrap_or_default(),
                row.id,
            );
            let session = JurySession {
                id: row.id,
                number: 0,
                categories: members
                    .iter()
                    .map(|c| SessionCategory {
                        id: c.id,
                        name: c.name.clone(),
                    })
                    .collect(),
                needed_technical: row.technical.unwrap_or(unit_needed.0),
                needed_performance: row.performance.unwrap_or(unit_needed.1),
                needed_dismounts: row.dismounts.unwrap_or(unit_needed.2),
                override_technical: row.technical,
                override_performance: row.performance,
                override_dismounts: row.dismounts,
            };
            (key, session)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed
        .into_iter()
        .enumerate()
        .map(|(i, (_, mut session))| {
            session.number = i as i64 + 1;
            session
        })
        .collect()
}

/// Checks the Durchgänge a competition file lists for a unit against the
/// unit's categories (names). `unit_label` is e.g. "Heat E EW" or "Final
/// Finale EW". Every category must be in exactly one session; an empty list
/// is the default session and always fine.
pub fn check_file_sessions(
    unit_label: &str,
    unit_categories: &[String],
    sessions: &[JurySessionConfig],
) -> Result<(), String> {
    if sessions.is_empty() {
        return Ok(());
    }
    if unit_categories.is_empty() {
        return Err(format!(
            "{unit_label}: it has no categories, so it can't be split into jury sessions."
        ));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for (i, session) in sessions.iter().enumerate() {
        let n = i + 1;
        if session.categories.is_empty() {
            return Err(format!("{unit_label}: jury session {n} has no categories."));
        }
        if [
            session.needed_technical,
            session.needed_performance,
            session.needed_dismounts,
        ]
        .into_iter()
        .flatten()
        .any(|n| n < 0)
        {
            return Err(format!(
                "{unit_label}: the jury deployments and needed judges can't be negative."
            ));
        }
        for name in session.categories.iter().map(|c| c.trim()) {
            if !unit_categories.iter().any(|c| c == name) {
                return Err(format!(
                    "{unit_label}: jury session {n} lists the category {name}, which isn't one of its categories."
                ));
            }
            if !seen.insert(name) {
                return Err(format!(
                    "{unit_label}: the category {name} is in more than one jury session."
                ));
            }
        }
    }
    if let Some(name) = unit_categories.iter().find(|c| !seen.contains(c.as_str())) {
        return Err(format!(
            "{unit_label}: the category {name} is in no jury session; every category must be in exactly one."
        ));
    }
    Ok(())
}

/// Replaces the unit's default session (see [`create_default_session`]) with
/// the Durchgänge of the competition file, which [`check_file_sessions`]
/// accepted; a list without entries leaves the default. Call it once the
/// unit's categories (a final's slots) exist.
pub async fn create_file_sessions(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    unit: JuryUnitId,
    sessions: &[JurySessionConfig],
) -> Result<(), HttpError> {
    if sessions.is_empty() {
        return Ok(());
    }
    let by_name: HashMap<String, CategoryId> = unit_categories(conn, kind, unit)
        .await?
        .into_iter()
        .map(|c| (c.name, c.id))
        .collect();
    let default = first_session(conn, unit).await?.ok_or_else(not_found)?;
    let (heat_id, final_id) = jury::unit_columns(kind, unit);
    for (i, session) in sessions.iter().enumerate() {
        let id = if i == 0 {
            default
        } else {
            let id = JurySessionId::new();
            sqlx::query!(
                "INSERT INTO jury_sessions (id, heat_id, final_id) VALUES (?, ?, ?)",
                id,
                heat_id,
                final_id,
            )
            .execute(&mut *conn)
            .await?;
            id
        };
        sqlx::query!(
            "UPDATE jury_sessions SET needed_technical = ?, needed_performance = ?, needed_dismounts = ? WHERE id = ?",
            session.needed_technical,
            session.needed_performance,
            session.needed_dismounts,
            id,
        )
        .execute(&mut *conn)
        .await?;
        for name in &session.categories {
            let category = by_name.get(name.trim()).ok_or_else(not_found)?;
            sqlx::query!(
                "INSERT INTO jury_session_categories (session_id, unit_id, category_id) VALUES (?, ?, ?)",
                id,
                unit,
                category,
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// The Durchgänge of the units that are split or have an override, as the
/// competition file lists them, by unit. Their order is the derived one
/// without the timeplan (category `order`, then name), so it is stable.
pub async fn sessions_for_file(
    conn: &mut SqliteConnection,
) -> sqlx::Result<HashMap<JuryUnitId, Vec<JurySessionConfig>>> {
    let units = sqlx::query!(
        r#"SELECT id as "id!: JuryUnitId", kind as "kind!: JuryUnitKind" FROM view_jury_unit"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let rows = load_session_rows(conn).await?;
    let memberships = load_memberships(conn).await?;
    let mut listed = HashMap::new();
    for unit in units {
        let own: Vec<SessionRow> = rows
            .iter()
            .filter(|r| r.unit_id == unit.id)
            .copied()
            .collect();
        let categories = unit_categories(conn, unit.kind, unit.id).await?;
        let sessions = assemble(unit.id, &categories, &|_| 0, &own, &memberships, (0, 0, 0));
        let overridden = sessions.iter().any(|s| {
            s.override_technical.is_some()
                || s.override_performance.is_some()
                || s.override_dismounts.is_some()
        });
        if sessions.len() > 1 || overridden {
            listed.insert(
                unit.id,
                sessions
                    .into_iter()
                    .map(|s| JurySessionConfig {
                        categories: s.categories.into_iter().map(|c| c.name).collect(),
                        needed_technical: s.override_technical,
                        needed_performance: s.override_performance,
                        needed_dismounts: s.override_dismounts,
                    })
                    .collect(),
            );
        }
    }
    Ok(listed)
}

/// The file's Durchgänge of every unit as text lines (`kind code json`), to
/// tell whether a file changes anything: no generated ids.
pub async fn snapshot(conn: &mut SqliteConnection) -> sqlx::Result<Vec<String>> {
    let sessions = sessions_for_file(conn).await?;
    let units = sqlx::query!(
        r#"SELECT id as "id!: JuryUnitId", kind as "kind!: String", code as "code!"
        FROM view_jury_unit ORDER BY kind, code"#
    )
    .fetch_all(conn)
    .await?;
    Ok(units
        .into_iter()
        .filter_map(|unit| {
            let sessions = sessions.get(&unit.id)?;
            Some(format!(
                "{} {} {}",
                unit.kind,
                unit.code,
                serde_json::to_string(sessions).unwrap_or_default()
            ))
        })
        .collect())
}

/// A requested Durchgang: an existing one (`id`) or a new one, with the
/// categories it holds.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct SessionSplit {
    #[serde(default)]
    pub id: Option<JurySessionId>,
    pub category_ids: Vec<CategoryId>,
}

/// What a save of the split changed in the assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct SplitResult {
    /// Kept assignments of removed Durchgänge, now in their target.
    pub moved: i64,
    /// Assignments of removed Durchgänge that were not kept.
    pub deleted: i64,
}

/// An assignment of a Durchgang a save of the split removes.
struct RemovedAssignment {
    session: JurySessionId,
    judge: JudgeId,
    task: JudgeTask,
    seat: i64,
}

fn refuse(message: &str) -> HttpError {
    HttpError::ErrorMessages(message.to_string())
}

fn not_found() -> HttpError {
    HttpError::StatusCode(StatusCode::NOT_FOUND)
}

/// Saves the split of a unit into sessions, in the caller's transaction (it
/// must roll back on an error). See ADR 0019.
///
/// The listed sessions replace the unit's: a listed `id` keeps its
/// assignments and overrides, an entry without one is new, the unit's
/// sessions that are not listed are removed. Of a removed session's
/// assignments only those in `keep` survive, in the listed session that
/// receives most of its categories (a tie: the earliest in the list); the
/// rest is deleted. A kept assignment keeps its seat if that is free in its
/// target (within the target's count), else gets the lowest free one; with
/// none within the count the save is refused (ADR 0020).
pub async fn set_sessions(
    conn: &mut SqliteConnection,
    kind: JuryUnitKind,
    unit: JuryUnitId,
    sessions: Vec<SessionSplit>,
    keep: Vec<JuryAssignmentId>,
) -> Result<SplitResult, HttpError> {
    if jury::unit_is_registrable(conn, kind, unit).await?.is_none() {
        return Err(not_found());
    }
    let categories = unit_categories(conn, kind, unit).await?;
    let existing: Vec<JurySessionId> = sqlx::query_scalar!(
        r#"SELECT id as "id!: JurySessionId" FROM jury_sessions WHERE unit_id = ?"#,
        unit
    )
    .fetch_all(&mut *conn)
    .await?;
    let existing_set: HashSet<JurySessionId> = existing.iter().copied().collect();

    let mut listed_ids = HashSet::new();
    for id in sessions.iter().filter_map(|s| s.id) {
        if !existing_set.contains(&id) {
            return Err(not_found());
        }
        if !listed_ids.insert(id) {
            return Err(refuse("Ein Durchgang ist doppelt aufgeführt."));
        }
    }
    if sessions.is_empty() {
        return Err(refuse("Es braucht mindestens einen Durchgang."));
    }
    let unit_ids: HashSet<CategoryId> = categories.iter().map(|c| c.id).collect();
    let mut seen = HashSet::new();
    for id in sessions.iter().flat_map(|s| &s.category_ids) {
        if !unit_ids.contains(id) || !seen.insert(*id) {
            return Err(refuse("Jede Kategorie gehört in genau einen Durchgang."));
        }
    }
    if seen.len() != unit_ids.len() {
        return Err(refuse("Jede Kategorie gehört in genau einen Durchgang."));
    }
    if sessions.iter().any(|s| s.category_ids.is_empty())
        && !(categories.is_empty() && sessions.len() == 1)
    {
        return Err(refuse("Ein Durchgang braucht mindestens eine Kategorie."));
    }

    // Every listed Durchgang has an id from here on.
    let listed: Vec<(JurySessionId, &SessionSplit)> = sessions
        .iter()
        .map(|s| (s.id.unwrap_or_else(JurySessionId::new), s))
        .collect();
    let removed: Vec<JurySessionId> = existing
        .iter()
        .copied()
        .filter(|id| !listed_ids.contains(id))
        .collect();

    // Where each removed session's kept assignments go.
    let current = session_of_category(conn, kind, unit).await?;
    let group_of: HashMap<CategoryId, usize> = listed
        .iter()
        .enumerate()
        .flat_map(|(i, (_, s))| s.category_ids.iter().map(move |c| (*c, i)))
        .collect();
    let mut target_of: HashMap<JurySessionId, JurySessionId> = HashMap::new();
    for session in &removed {
        let mut votes = vec![0usize; listed.len()];
        for (category, owner) in &current {
            if owner == session
                && let Some(group) = group_of.get(category)
            {
                votes[*group] += 1;
            }
        }
        // The earliest group with most votes.
        let best = votes
            .iter()
            .enumerate()
            .fold(
                (0, 0),
                |(bi, bv), (i, v)| if *v > bv { (i, *v) } else { (bi, bv) },
            )
            .0;
        target_of.insert(*session, listed[best].0);
    }

    // The assignments of the removed sessions, and the ones kept.
    let mut of_removed: HashMap<JuryAssignmentId, RemovedAssignment> = HashMap::new();
    for session in &removed {
        for row in sqlx::query!(
            r#"SELECT id as "id!: JuryAssignmentId", judge_id as "judge_id!: JudgeId",
                task as "task!: JudgeTask", seat as "seat!: i64"
            FROM jury_assignments WHERE session_id = ?"#,
            session
        )
        .fetch_all(&mut *conn)
        .await?
        {
            of_removed.insert(
                row.id,
                RemovedAssignment {
                    session: *session,
                    judge: row.judge_id,
                    task: row.task,
                    seat: row.seat,
                },
            );
        }
    }
    let keep: HashSet<JuryAssignmentId> = keep.into_iter().collect();
    if keep.iter().any(|id| !of_removed.contains_key(id)) {
        return Err(refuse(
            "Diese Einteilung gehört nicht zu einem zusammengelegten Durchgang.",
        ));
    }

    // One task per judge and Durchgang, also with what the target holds.
    let mut judges: HashMap<(JurySessionId, JudgeId), i64> = HashMap::new();
    for (id, _) in &listed {
        for judge in sqlx::query_scalar!(
            r#"SELECT judge_id as "judge_id!: JudgeId" FROM jury_assignments WHERE session_id = ?"#,
            id
        )
        .fetch_all(&mut *conn)
        .await?
        {
            *judges.entry((*id, judge)).or_default() += 1;
        }
    }
    for id in &keep {
        let moved = &of_removed[id];
        *judges
            .entry((target_of[&moved.session], moved.judge))
            .or_default() += 1;
    }
    if let Some((_, judge)) = judges.iter().find(|(_, n)| **n > 1).map(|(k, _)| *k) {
        let name = sqlx::query!(
            r#"SELECT firstname as "firstname!", lastname as "lastname!" FROM view_judge WHERE id = ?"#,
            judge
        )
        .fetch_one(&mut *conn)
        .await?;
        return Err(HttpError::ErrorMessages(format!(
            "Eine JurorIn kann in einem Durchgang nur eine Aufgabe haben: {} {} wäre mehrfach eingeteilt.",
            name.firstname, name.lastname
        )));
    }

    // Apply.
    sqlx::query!(
        "DELETE FROM jury_session_categories WHERE unit_id = ?",
        unit
    )
    .execute(&mut *conn)
    .await?;
    let (heat_id, final_id) = jury::unit_columns(kind, unit);
    for (id, split) in &listed {
        if split.id.is_none() {
            sqlx::query!(
                "INSERT INTO jury_sessions (id, heat_id, final_id) VALUES (?, ?, ?)",
                id,
                heat_id,
                final_id,
            )
            .execute(&mut *conn)
            .await?;
        }
        for category in &split.category_ids {
            sqlx::query!(
                "INSERT INTO jury_session_categories (session_id, unit_id, category_id) VALUES (?, ?, ?)",
                id,
                unit,
                category,
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    let mut result = SplitResult {
        moved: 0,
        deleted: 0,
    };
    for id in of_removed.keys().filter(|id| !keep.contains(id)) {
        sqlx::query!("DELETE FROM jury_assignments WHERE id = ?", id)
            .execute(&mut *conn)
            .await?;
        result.deleted += 1;
    }
    // Kept assignments keep their seat if it is free in the target (taken
    // ones include those moved before), else get its lowest free one; none
    // within the target's count refuses the save. Order: old seat, then id.
    let mut kept: Vec<(&JuryAssignmentId, &RemovedAssignment)> = of_removed
        .iter()
        .filter(|(id, _)| keep.contains(id))
        .collect();
    kept.sort_by_key(|(id, moved)| (moved.seat, **id));
    for (id, moved) in kept {
        let target = target_of[&moved.session];
        let needed = session_needed(conn, target, moved.task).await?;
        let used = used_seats(conn, target, moved.task).await?;
        let lowest_free = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        let seat = if !used.contains(&moved.seat) && moved.seat <= needed {
            moved.seat
        } else {
            lowest_free
        };
        if seat > needed {
            let label = if listed.len() == 1 {
                sqlx::query_scalar!(
                    r#"SELECT name as "name!" FROM view_jury_unit WHERE id = ?"#,
                    unit
                )
                .fetch_one(&mut *conn)
                .await?
            } else {
                let group = listed.iter().find(|(id, _)| *id == target).map(|(_, s)| *s);
                let mut names: Vec<&UnitCategory> = categories
                    .iter()
                    .filter(|c| group.is_some_and(|g| g.category_ids.contains(&c.id)))
                    .collect();
                names.sort_by(|a, b| {
                    (a.order.unwrap_or(i64::MAX), &a.name)
                        .cmp(&(b.order.unwrap_or(i64::MAX), &b.name))
                });
                names
                    .into_iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let name = sqlx::query!(
                r#"SELECT firstname as "firstname!", lastname as "lastname!" FROM view_judge WHERE id = ?"#,
                moved.judge
            )
            .fetch_one(&mut *conn)
            .await?;
            return Err(HttpError::ErrorMessages(format!(
                "Im Durchgang {label} gibt es nicht genug freie Plätze für {}: {} {} bekommt keinen Platz.",
                task_name(moved.task),
                name.firstname,
                name.lastname
            )));
        }
        sqlx::query!(
            "UPDATE jury_assignments SET session_id = ?, seat = ? WHERE id = ?",
            target,
            seat,
            id
        )
        .execute(&mut *conn)
        .await?;
        result.moved += 1;
    }
    for session in &removed {
        sqlx::query!("DELETE FROM jury_sessions WHERE id = ?", session)
            .execute(&mut *conn)
            .await?;
    }
    Ok(result)
}

/// Sets (or, with `None`, clears) a session's needed-judge overrides.
pub async fn set_session_needed(
    conn: &mut SqliteConnection,
    id: JurySessionId,
    technical: Option<i64>,
    performance: Option<i64>,
    dismounts: Option<i64>,
) -> Result<(), HttpError> {
    if [technical, performance, dismounts]
        .into_iter()
        .flatten()
        .any(|n| n < 0)
    {
        return Err(refuse(
            "Einsätze und benötigte JurorInnen dürfen nicht negativ sein.",
        ));
    }
    let result = sqlx::query!(
        "UPDATE jury_sessions SET needed_technical = ?, needed_performance = ?, needed_dismounts = ? WHERE id = ?",
        technical,
        performance,
        dismounts,
        id,
    )
    .execute(conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(not_found());
    }
    Ok(())
}

/// The seat's name: the task's letter (T, P or A), then the number.
pub fn seat_label(task: JudgeTask, seat: i64) -> String {
    let letter = match task {
        JudgeTask::Technical => 'T',
        JudgeTask::Performance => 'P',
        JudgeTask::Dismounts => 'A',
    };
    format!("{letter}{seat}")
}

fn task_name(task: JudgeTask) -> &'static str {
    match task {
        JudgeTask::Technical => "Technik",
        JudgeTask::Performance => "Performance",
        JudgeTask::Dismounts => "Abstiege",
    }
}

/// The unit a session belongs to. Unknown session: 404.
async fn session_unit(
    conn: &mut SqliteConnection,
    session: JurySessionId,
) -> Result<(JuryUnitKind, JuryUnitId), HttpError> {
    let unit = sqlx::query!(
        r#"SELECT heat_id as "heat_id: crate::ids::HeatId", final_id as "final_id: crate::ids::FinalId"
        FROM jury_sessions WHERE id = ?"#,
        session
    )
    .fetch_optional(conn)
    .await?
    .ok_or_else(not_found)?;
    match (unit.heat_id, unit.final_id) {
        (Some(heat), _) => Ok((JuryUnitKind::Heat, JuryUnitId::from(heat))),
        (None, Some(final_id)) => Ok((JuryUnitKind::Final, JuryUnitId::from(final_id))),
        (None, None) => Err(not_found()),
    }
}

/// How many judges the session needs for the task (its seat count): the
/// session's override, else its unit's count. Unknown session: 404.
pub async fn session_needed(
    conn: &mut SqliteConnection,
    session: JurySessionId,
    task: JudgeTask,
) -> Result<i64, HttpError> {
    let row = sqlx::query!(
        r#"
        SELECT s.needed_technical, s.needed_performance, s.needed_dismounts,
            u.needed_technical as "unit_technical!: i64",
            u.needed_performance as "unit_performance!: i64",
            u.needed_dismounts as "unit_dismounts!: i64"
        FROM jury_sessions s
        JOIN view_jury_unit u ON u.id = s.unit_id
        WHERE s.id = ?
        "#,
        session
    )
    .fetch_optional(conn)
    .await?
    .ok_or_else(not_found)?;
    Ok(match task {
        JudgeTask::Technical => row.needed_technical.unwrap_or(row.unit_technical),
        JudgeTask::Performance => row.needed_performance.unwrap_or(row.unit_performance),
        JudgeTask::Dismounts => row.needed_dismounts.unwrap_or(row.unit_dismounts),
    })
}

/// The seat numbers of the session's task that somebody sits on (migrated
/// seats beyond the count included).
async fn used_seats(
    conn: &mut SqliteConnection,
    session: JurySessionId,
    task: JudgeTask,
) -> sqlx::Result<HashSet<i64>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT seat as "seat!: i64" FROM jury_assignments WHERE session_id = ? AND task = ?"#,
        session,
        task
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .collect())
}

fn no_such_seat(task: JudgeTask, seat: i64) -> HttpError {
    HttpError::ErrorMessages(format!(
        "Platz {} gibt es in diesem Durchgang nicht.",
        seat_label(task, seat)
    ))
}

/// Assigns a judge a task and seat in a session; the unit is the session's.
/// Without a `seat` the lowest free one is taken. Unknown judge or session:
/// 404. Untrained people only count dismounts. A judge sits once per
/// session, but may sit in several sessions of a unit. Only seats within the
/// session's needed count exist; a taken seat is a conflict, and no free seat
/// is refused (ADR 0020).
pub async fn assign(
    conn: &mut SqliteConnection,
    judge: JudgeId,
    session: JurySessionId,
    task: JudgeTask,
    seat: Option<i64>,
) -> Result<JuryAssignment, HttpError> {
    let is_trained = sqlx::query_scalar!("SELECT is_trained FROM judge WHERE id = ?", judge)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(not_found)?;
    let (unit_kind, unit_id) = session_unit(conn, session).await?;
    if !is_trained && task != JudgeTask::Dismounts {
        return Err(refuse(
            "Nicht geschulte Personen können nur Abstiege zählen.",
        ));
    }
    let needed = session_needed(conn, session, task).await?;
    let used = used_seats(conn, session, task).await?;
    let seat = match seat {
        Some(seat) => {
            if seat < 1 || seat > needed {
                return Err(no_such_seat(task, seat));
            }
            if used.contains(&seat) {
                return Err(HttpError::Conflict(format!(
                    "Platz {} ist schon besetzt.",
                    seat_label(task, seat)
                )));
            }
            seat
        }
        None => {
            let seat = (1..).find(|n| !used.contains(n)).unwrap_or(1);
            if seat > needed {
                return Err(HttpError::ErrorMessages(format!(
                    "Alle Plätze für {} sind besetzt.",
                    task_name(task)
                )));
            }
            seat
        }
    };
    let id = JuryAssignmentId::new();
    let (heat_id, final_id) = jury::unit_columns(unit_kind, unit_id);
    let result = sqlx::query!(
        "INSERT INTO jury_assignments (id, judge_id, heat_id, final_id, session_id, task, seat) VALUES (?, ?, ?, ?, ?, ?, ?)",
        id,
        judge,
        heat_id,
        final_id,
        session,
        task,
        seat,
    )
    .execute(conn)
    .await;
    match result {
        Ok(_) => Ok(JuryAssignment {
            id,
            judge_id: judge,
            session_id: session,
            unit_kind,
            unit_id,
            task,
            seat,
            seat_label: seat_label(task, seat),
        }),
        Err(e) if jury::is_unique_violation(&e) => {
            Err(refuse("Die JurorIn ist hier schon eingeteilt."))
        }
        Err(e) => Err(e.into()),
    }
}

/// What [`seat_judge`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeatOutcome {
    /// The judge had no seat in the Durchgang and the seat was free.
    Assigned,
    /// The judge moved from another seat to the free seat.
    Moved,
    /// The judge and the seat's occupant changed seats.
    Swapped,
    /// The judge moved to the occupied seat; the occupant could not take the
    /// judge's old seat and was removed.
    MovedReplaced,
    /// The judge had no seat and replaced the occupant, who was removed.
    Replaced,
    /// The judge already sat on the seat.
    Unchanged,
}

/// Whether the occupant O of the seat a judge moves to takes the judge's old
/// seat (task `a`), the judge now being on a seat of task `b`: on the same
/// task, or when O is registered for task `a` of the unit, and O may judge
/// `a` (trained, or `a` is dismounts).
fn occupant_follows(
    task_a: JudgeTask,
    task_b: JudgeTask,
    o_registered_for_a: bool,
    o_trained: bool,
) -> bool {
    (task_a == task_b || o_registered_for_a) && (o_trained || task_a == JudgeTask::Dismounts)
}

/// Puts a judge on a seat of a session: assigns, moves, swaps or replaces,
/// as the state of the judge and the seat requires, in the caller's
/// transaction (it must roll back on an error). A judge sits once per
/// session, so a judge on another seat moves; the seat's occupant then takes
/// the judge's old seat (see [`occupant_follows`]) or is removed. A judge
/// without a seat replaces the occupant. Only seats within the needed count
/// can be chosen; an old seat beyond it can be the occupant's new one.
pub async fn seat_judge(
    conn: &mut SqliteConnection,
    judge: JudgeId,
    session: JurySessionId,
    task: JudgeTask,
    seat: i64,
) -> Result<SeatOutcome, HttpError> {
    let is_trained = sqlx::query_scalar!("SELECT is_trained FROM judge WHERE id = ?", judge)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(not_found)?;
    let (unit_kind, unit_id) = session_unit(conn, session).await?;
    let needed = session_needed(conn, session, task).await?;
    if seat < 1 || seat > needed {
        return Err(no_such_seat(task, seat));
    }
    let current = sqlx::query!(
        r#"SELECT id as "id!: JuryAssignmentId", task as "task!: JudgeTask", seat as "seat!: i64"
        FROM jury_assignments WHERE judge_id = ? AND session_id = ?"#,
        judge,
        session
    )
    .fetch_optional(&mut *conn)
    .await?;
    if current
        .as_ref()
        .is_some_and(|c| c.task == task && c.seat == seat)
    {
        return Ok(SeatOutcome::Unchanged);
    }
    if !is_trained && task != JudgeTask::Dismounts {
        return Err(refuse(
            "Nicht geschulte Personen können nur Abstiege zählen.",
        ));
    }
    let occupant = sqlx::query!(
        r#"SELECT id as "id!: JuryAssignmentId", judge_id as "judge_id!: JudgeId"
        FROM jury_assignments WHERE session_id = ? AND task = ? AND seat = ?"#,
        session,
        task,
        seat
    )
    .fetch_optional(&mut *conn)
    .await?;
    let (heat_id, final_id) = jury::unit_columns(unit_kind, unit_id);

    // The occupant's row is removed first, so the seat is free for the judge.
    if let Some(occupant) = &occupant {
        sqlx::query!("DELETE FROM jury_assignments WHERE id = ?", occupant.id)
            .execute(&mut *conn)
            .await?;
    }
    let Some(current) = current else {
        sqlx::query!(
            "INSERT INTO jury_assignments (id, judge_id, heat_id, final_id, session_id, task, seat) VALUES (?, ?, ?, ?, ?, ?, ?)",
            JuryAssignmentId::new(),
            judge,
            heat_id,
            final_id,
            session,
            task,
            seat,
        )
        .execute(&mut *conn)
        .await?;
        return Ok(if occupant.is_some() {
            SeatOutcome::Replaced
        } else {
            SeatOutcome::Assigned
        });
    };
    sqlx::query!(
        "UPDATE jury_assignments SET task = ?, seat = ? WHERE id = ?",
        task,
        seat,
        current.id
    )
    .execute(&mut *conn)
    .await?;
    let Some(occupant) = occupant else {
        return Ok(SeatOutcome::Moved);
    };
    let o_trained = sqlx::query_scalar!(
        "SELECT is_trained FROM judge WHERE id = ?",
        occupant.judge_id
    )
    .fetch_one(&mut *conn)
    .await?;
    let o_registered = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM judge_registration
        WHERE judge_id = ? AND unit_id = ? AND task = ?"#,
        occupant.judge_id,
        unit_id,
        current.task
    )
    .fetch_one(&mut *conn)
    .await?
        > 0;
    if !occupant_follows(current.task, task, o_registered, o_trained) {
        return Ok(SeatOutcome::MovedReplaced);
    }
    sqlx::query!(
        "INSERT INTO jury_assignments (id, judge_id, heat_id, final_id, session_id, task, seat) VALUES (?, ?, ?, ?, ?, ?, ?)",
        occupant.id,
        occupant.judge_id,
        heat_id,
        final_id,
        session,
        current.task,
        current.seat,
    )
    .execute(&mut *conn)
    .await?;
    Ok(SeatOutcome::Swapped)
}

/// The unit a session belongs to. Unknown session: 404.
pub async fn unit_of_session(
    conn: &mut SqliteConnection,
    session: JurySessionId,
) -> Result<JuryUnitId, HttpError> {
    Ok(session_unit(conn, session).await?.1)
}

/// The effective needed count of every (session, task) of a unit, taken
/// before an edit.
pub struct NeededSnapshot(Vec<(JurySessionId, JudgeTask, i64)>);

/// Takes the unit's [`NeededSnapshot`].
pub async fn needed_snapshot(
    conn: &mut SqliteConnection,
    unit: JuryUnitId,
) -> sqlx::Result<NeededSnapshot> {
    let sessions: Vec<JurySessionId> = sqlx::query_scalar!(
        r#"SELECT id as "id!: JurySessionId" FROM jury_sessions WHERE unit_id = ? ORDER BY id"#,
        unit
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut counts = vec![];
    for session in sessions {
        for task in [
            JudgeTask::Technical,
            JudgeTask::Performance,
            JudgeTask::Dismounts,
        ] {
            let needed = match session_needed(conn, session, task).await {
                Ok(needed) => needed,
                Err(HttpError::DBError(e)) => return Err(e),
                Err(_) => continue,
            };
            counts.push((session, task, needed));
        }
    }
    Ok(NeededSnapshot(counts))
}

/// A split unit's Durchgang as people read it: its categories' names, `None`
/// for an unsplit unit.
async fn session_label(
    conn: &mut SqliteConnection,
    session: JurySessionId,
) -> Result<Option<String>, HttpError> {
    let (kind, unit) = session_unit(conn, session).await?;
    let sessions: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM jury_sessions WHERE unit_id = ?"#,
        unit
    )
    .fetch_one(&mut *conn)
    .await?;
    if sessions < 2 {
        return Ok(None);
    }
    let owner = session_of_category(conn, kind, unit).await?;
    let mut names: Vec<UnitCategory> = unit_categories(conn, kind, unit)
        .await?
        .into_iter()
        .filter(|c| owner.get(&c.id) == Some(&session))
        .collect();
    names.sort_by(|a, b| {
        (a.order.unwrap_or(i64::MAX), &a.name).cmp(&(b.order.unwrap_or(i64::MAX), &b.name))
    });
    Ok(Some(
        names
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(", "),
    ))
}

/// After an edit, in the same transaction: refuses (409) when an effective
/// needed count went down to below its highest occupied seat. Only a real
/// lowering counts, so saving other fields of a unit whose migrated seats
/// already exceed its count still works.
pub async fn ensure_seats_fit(
    conn: &mut SqliteConnection,
    before: NeededSnapshot,
) -> Result<(), HttpError> {
    for (session, task, old) in before.0 {
        let new = session_needed(conn, session, task).await?;
        if new >= old {
            continue;
        }
        let highest = sqlx::query!(
            r#"
            SELECT a.seat as "seat!: i64", j.firstname as "firstname!", j.lastname as "lastname!"
            FROM jury_assignments a
            JOIN view_judge j ON j.id = a.judge_id
            WHERE a.session_id = ? AND a.task = ?
            ORDER BY a.seat DESC LIMIT 1
            "#,
            session,
            task
        )
        .fetch_optional(&mut *conn)
        .await?;
        let Some(highest) = highest.filter(|h| h.seat > new) else {
            continue;
        };
        let prefix = session_label(conn, session)
            .await?
            .map(|label| format!("{label}: "))
            .unwrap_or_default();
        return Err(HttpError::Conflict(format!(
            "{prefix}Platz {} ist noch besetzt ({} {}). Erst den Platz freigeben, dann den Bedarf auf {new} senken.",
            seat_label(task, highest.seat),
            highest.firstname,
            highest.lastname,
        )));
    }
    Ok(())
}

/// Removes a judge from a session.
pub async fn unassign(
    conn: &mut SqliteConnection,
    judge: JudgeId,
    session: JurySessionId,
) -> sqlx::Result<()> {
    sqlx::query!(
        "DELETE FROM jury_assignments WHERE judge_id = ? AND session_id = ?",
        judge,
        session,
    )
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use JudgeTask::*;

    #[test]
    fn the_occupant_follows_on_the_same_task_or_when_registered_and_allowed() {
        // Same task: no registration needed.
        assert!(occupant_follows(Technical, Technical, false, true));
        // Another task: registered for the judge's old task.
        assert!(occupant_follows(Technical, Performance, true, true));
        assert!(!occupant_follows(Technical, Performance, false, true));
        // An untrained occupant only follows onto dismounts.
        assert!(!occupant_follows(Technical, Performance, true, false));
        assert!(occupant_follows(Dismounts, Technical, true, false));
        assert!(!occupant_follows(Dismounts, Technical, false, false));
    }

    #[test]
    fn seats_are_named_by_task_letter_and_number() {
        assert_eq!(seat_label(Technical, 3), "T3");
        assert_eq!(seat_label(Performance, 1), "P1");
        assert_eq!(seat_label(Dismounts, 12), "A12");
    }
}
