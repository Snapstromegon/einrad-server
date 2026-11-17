//! Jury sessions (Jurydurchgänge, ADR 0019): the set of a jury unit's
//! categories that one jury judges. A unit has one by default; only the jury
//! lead splits it. Registration stays per unit.

use std::collections::HashMap;

use sqlx::SqliteConnection;

use crate::{
    ids::{CategoryId, JudgeId, JuryAssignmentId, JurySessionId, JuryUnitId},
    jury::{self, JudgeTask, JuryUnitKind},
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

/// Assigns a judge a task in a session. Fails with a unique violation if the
/// judge already sits in it.
pub async fn assign(
    conn: &mut SqliteConnection,
    judge: JudgeId,
    kind: JuryUnitKind,
    unit: JuryUnitId,
    session: JurySessionId,
    task: JudgeTask,
) -> sqlx::Result<JuryAssignmentId> {
    let id = JuryAssignmentId::new();
    let (heat_id, final_id) = jury::unit_columns(kind, unit);
    sqlx::query!(
        "INSERT INTO jury_assignments (id, judge_id, heat_id, final_id, session_id, task) VALUES (?, ?, ?, ?, ?, ?)",
        id,
        judge,
        heat_id,
        final_id,
        session,
        task,
    )
    .execute(conn)
    .await?;
    Ok(id)
}

/// Removes a judge from every session of a unit.
pub async fn unassign_from_unit(
    conn: &mut SqliteConnection,
    judge: JudgeId,
    unit: JuryUnitId,
) -> sqlx::Result<()> {
    sqlx::query!(
        "DELETE FROM jury_assignments WHERE judge_id = ? AND unit_id = ?",
        judge,
        unit,
    )
    .execute(conn)
    .await?;
    Ok(())
}
