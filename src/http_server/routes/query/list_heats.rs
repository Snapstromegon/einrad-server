use crate::ids::{CategoryId, FinalId, HeatId};
use std::collections::HashMap;

use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    act_type::ActType,
    heats::Discipline,
    http_server::{ClientError, HttpError},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct HeatFinal {
    id: FinalId,
    /// The readable name.
    name: String,
    /// The short code.
    code: String,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Heat {
    id: HeatId,
    /// The readable name, unique, e.g. "Einzel männlich – Expert 1 (U17–U21)".
    name: String,
    /// The short code, unique, e.g. "E EM 1".
    code: String,
    description: Option<String>,
    /// Its categories, in category order. Empty until the admin assigns
    /// them.
    categories: Vec<String>,
    /// The same categories by id, in the same order.
    category_ids: Vec<CategoryId>,
    /// The act type its categories share; `None` without categories.
    act_type: Option<ActType>,
    /// Whether its categories are singles male; `None` without categories.
    is_single_male: Option<bool>,
    /// The finals whose slots it fills; at most one.
    finals: Vec<HeatFinal>,
    /// The fewest acts it needs for its Vorlauf.
    threshold: i64,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    skip: bool,
    /// Its acts at the moment, without withdrawn ones.
    act_count: i64,
    /// Whether the tool suggests skipping its Vorlauf: it has fewer acts than
    /// its threshold; never for groups, which start in their final. The
    /// organizer decides.
    skip_suggested: bool,
}

/// List the heats: groups of categories of one discipline whose best acts
/// fill a final's slots. In the order of their first category; heats without
/// categories last.
#[utoipa::path(
    get,
    tags=["query", "heat"],
    path="/list_heats",
    responses(
        (status=200, content_type="application/json", body=Vec<Heat>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_heats(
    Extension(db): Extension<ReloadableSqlite>,
) -> Result<Json<Vec<Heat>>, HttpError> {
    let db = db.get().await.clone();
    // One read transaction, so the heats and their categories match.
    let mut tx = db.begin().await?;
    let mut categories: HashMap<HeatId, Vec<(CategoryId, String, Discipline)>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT heat_id as "heat_id!: HeatId", id as "id!: CategoryId", name as "name!", act_type as "act_type: ActType", is_single_male
        FROM categories
        WHERE heat_id IS NOT NULL
        ORDER BY "order" NULLS FIRST, name
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        categories
            .entry(row.heat_id)
            .or_default()
            .push((row.id, row.name, Discipline::new(row.act_type, row.is_single_male)));
    }
    let mut finals: HashMap<HeatId, Vec<HeatFinal>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT DISTINCT final_slots.heat_id as "heat_id!: HeatId", finals.id as "final_id!: FinalId", finals.name as "final_name!", finals.code as "final_code!"
        FROM final_slots
        JOIN finals ON finals.id = final_slots.final_id
        ORDER BY finals.name
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        finals.entry(row.heat_id).or_default().push(HeatFinal {
            id: row.final_id,
            name: row.final_name,
            code: row.final_code,
        });
    }
    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!: HeatId",
            name as "name!",
            code as "code!",
            description,
            threshold,
            skip,
            (
                SELECT COUNT(*) FROM view_act
                WHERE withdrawn_at IS NULL AND category_id IN (SELECT id FROM categories WHERE heat_id = heats.id)
            ) as "act_count!: i64"
        FROM heats
        ORDER BY
            (SELECT MIN("order") FROM categories WHERE heat_id = heats.id) IS NULL,
            (SELECT MIN("order") FROM categories WHERE heat_id = heats.id),
            name
        "#
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let categories = categories.remove(&row.id).unwrap_or_default();
                let discipline = categories.first().map(|(_, _, d)| *d);
                Heat {
                    finals: finals.remove(&row.id).unwrap_or_default(),
                    id: row.id,
                    category_ids: categories.iter().map(|(id, _, _)| *id).collect(),
                    categories: categories.into_iter().map(|(_, name, _)| name).collect(),
                    act_type: discipline.map(|d| d.act_type),
                    is_single_male: discipline.map(|d| d.single_male),
                    name: row.name,
                    code: row.code,
                    description: row.description,
                    threshold: row.threshold,
                    skip: row.skip,
                    act_count: row.act_count,
                    skip_suggested: !discipline.is_some_and(|d| d.is_group())
                        && row.act_count < row.threshold,
                }
            })
            .collect(),
    ))
}
