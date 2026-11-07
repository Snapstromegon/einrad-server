use std::collections::HashMap;

use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    finals::{self, SlotGroup},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Final {
    name: String,
    description: Option<String>,
    act_type: ActType,
    is_single_male: bool,
    order: i64,
    einfahrzeit_seconds: i64,
    act_duration_seconds: i64,
    judge_duration_seconds: i64,
    /// The reserved slots in start order, e.g. 5 × "U11–U15".
    slot_groups: Vec<SlotGroup>,
    /// The categories whose acts can be placed in the slots.
    categories: Vec<String>,
    /// The slots in start order, with their placed acts.
    slots: Vec<FinalSlot>,
    /// Whether the placement is published: the timeplan shows the acts.
    placement_published: bool,
    /// Whether a slot has started: the slot groups can't change any more.
    started: bool,
    /// The timeplan block it runs in, if it is planned.
    timeplan_id: Option<i64>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct FinalSlot {
    id: Uuid,
    /// "3. Finalist (U17–U21)".
    label: String,
    group_label: String,
    /// Counts within the group.
    number: i64,
    /// The placed act. Before the placement is published, only for who
    /// manages results.
    act_id: Option<Uuid>,
    started: bool,
}

/// List the finals, in order.
#[utoipa::path(
    get,
    tags=["query", "final"],
    path="/list_finals",
    responses(
        (status=200, content_type="application/json", body=Vec<Final>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_finals(
    Extension(db): Extension<ReloadableSqlite>,
    results: Option<Granted<permit::ManageResults>>,
) -> Result<Json<Vec<Final>>, HttpError> {
    let db = db.get().await.clone();
    // One read transaction, so the finals and their slots match.
    let mut tx = db.begin().await?;
    let mut slots: HashMap<String, Vec<FinalSlot>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            final_slots.id as "id!: Uuid",
            final_name,
            group_label,
            number,
            act_id as "act_id: Uuid",
            started_at IS NOT NULL as "started!: bool",
            finals.placement_published
        FROM final_slots
        JOIN finals ON finals.name = final_slots.final_name
        ORDER BY final_name, position
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        slots.entry(row.final_name).or_default().push(FinalSlot {
            id: row.id,
            label: finals::finalist_label(row.number, &row.group_label),
            group_label: row.group_label,
            number: row.number,
            act_id: row
                .act_id
                .filter(|_| results.is_some() || row.placement_published),
            started: row.started,
        });
    }
    let mut categories: HashMap<String, Vec<String>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT final_name, category
        FROM final_categories
        LEFT JOIN categories ON categories.name = final_categories.category
        ORDER BY final_name, categories."order", category
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        categories
            .entry(row.final_name)
            .or_default()
            .push(row.category);
    }
    let rows = sqlx::query!(
        r#"
        SELECT
            name as "name!",
            description,
            act_type as "act_type: ActType",
            is_single_male,
            "order",
            einfahrzeit_seconds,
            act_duration_seconds,
            judge_duration_seconds,
            placement_published,
            (SELECT id FROM timeplan WHERE timeplan.final_name = finals.name) as "timeplan_id: i64"
        FROM finals
        ORDER BY "order", name
        "#
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let slots = slots.remove(&row.name).unwrap_or_default();
                Final {
                    slot_groups: finals::group_slots(
                        slots.iter().map(|slot| slot.group_label.as_str()),
                    ),
                    started: slots.iter().any(|slot| slot.started),
                    slots,
                    categories: categories.remove(&row.name).unwrap_or_default(),
                    placement_published: row.placement_published,
                    name: row.name,
                    description: row.description,
                    act_type: row.act_type,
                    is_single_male: row.is_single_male,
                    order: row.order,
                    einfahrzeit_seconds: row.einfahrzeit_seconds,
                    act_duration_seconds: row.act_duration_seconds,
                    judge_duration_seconds: row.judge_duration_seconds,
                    timeplan_id: row.timeplan_id,
                }
            })
            .collect(),
    ))
}
