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
pub struct Heat {
    name: String,
    description: Option<String>,
    /// Its categories, in category order. Empty until the admin assigns
    /// them.
    categories: Vec<String>,
    /// The act type its categories share; `None` without categories.
    act_type: Option<ActType>,
    /// Whether its categories are singles male; `None` without categories.
    is_single_male: Option<bool>,
    /// The finals whose slots it fills, by name; at most one.
    finals: Vec<String>,
    /// The fewest acts it needs for its Vorlauf.
    threshold: i64,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    skip: bool,
    /// Its acts at the moment, without withdrawn ones.
    act_count: i64,
    /// Whether the tool suggests skipping its Vorlauf: it has fewer acts than
    /// its threshold. The organizer decides.
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
    let mut categories: HashMap<String, Vec<(String, Discipline)>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT heat as "heat!", name as "name!", act_type as "act_type: ActType", is_single_male
        FROM categories
        WHERE heat IS NOT NULL
        ORDER BY "order", name
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        categories
            .entry(row.heat)
            .or_default()
            .push((row.name, Discipline::new(row.act_type, row.is_single_male)));
    }
    let mut finals: HashMap<String, Vec<String>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT DISTINCT heat as "heat!", final_name
        FROM final_slots
        ORDER BY final_name
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        finals.entry(row.heat).or_default().push(row.final_name);
    }
    let rows = sqlx::query!(
        r#"
        SELECT
            name as "name!",
            description,
            threshold,
            skip,
            (
                SELECT COUNT(*) FROM view_act
                WHERE category IN (SELECT name FROM categories WHERE heat = heats.name)
            ) as "act_count!: i64"
        FROM heats
        ORDER BY
            (SELECT MIN("order") FROM categories WHERE heat = heats.name) IS NULL,
            (SELECT MIN("order") FROM categories WHERE heat = heats.name),
            name
        "#
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let categories = categories.remove(&row.name).unwrap_or_default();
                let discipline = categories.first().map(|(_, d)| *d);
                Heat {
                    finals: finals.remove(&row.name).unwrap_or_default(),
                    categories: categories.into_iter().map(|(name, _)| name).collect(),
                    act_type: discipline.map(|d| d.act_type),
                    is_single_male: discipline.map(|d| d.single_male),
                    name: row.name,
                    description: row.description,
                    threshold: row.threshold,
                    skip: row.skip,
                    act_count: row.act_count,
                    skip_suggested: row.act_count < row.threshold,
                }
            })
            .collect(),
    ))
}
