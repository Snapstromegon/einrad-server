use std::collections::HashMap;

use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    heats::Discipline as HeatDiscipline,
    http_server::{
        ClientError, HttpError, extractor::auth::Granted, routes::http_types::ActParticipant,
    },
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// The competition's structure: its disciplines in act type order (single
/// female, single male, pair, small group, large group).
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Structure {
    disciplines: Vec<Discipline>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Discipline {
    act_type: ActType,
    /// Only singles are male.
    is_single_male: bool,
    /// Its acts, without withdrawn ones.
    act_count: i64,
    /// Its heats (Vorlaufklassen), in the order of their first category.
    heats: Vec<HeatNode>,
    /// Its categories that belong to no heat, in category order.
    without_heat: Vec<CategoryNode>,
    /// Its acts without a category: incomplete groups, groups with a type
    /// conflict and withdrawn acts.
    without_category: ActsNode,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct HeatNode {
    name: String,
    description: Option<String>,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    skip: bool,
    /// The final it feeds, if it feeds one.
    final_name: Option<String>,
    /// Its slots in that final.
    slots: Option<i64>,
    /// Its acts, without withdrawn ones.
    act_count: i64,
    categories: Vec<CategoryNode>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CategoryNode {
    name: String,
    description: Option<String>,
    /// Its acts, without withdrawn ones; they are all in `acts`.
    act_count: i64,
    /// In timeplan start order, acts not yet placed after the others by name.
    acts: Vec<ActNode>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ActsNode {
    /// Without withdrawn acts.
    act_count: i64,
    /// Including withdrawn ones, in timeplan start order.
    acts: Vec<ActNode>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ActNode {
    id: Uuid,
    /// The Kür's name; may be empty.
    name: String,
    /// Groups only.
    group_name: Option<String>,
    /// The riders, regular ones first.
    participants: Vec<ActParticipant>,
    withdrawn: bool,
    /// The start order within the category; `None` while not placed.
    order: Option<i64>,
}

struct CategoryRow {
    name: String,
    description: Option<String>,
    discipline: HeatDiscipline,
    heat: Option<String>,
}

/// An act with the category it ranks in, if any.
struct ActRow {
    category: Option<String>,
    discipline: HeatDiscipline,
    node: ActNode,
}

/// The disciplines in act type order.
const DISCIPLINES: [(ActType, bool); 5] = [
    (ActType::Single, false),
    (ActType::Single, true),
    (ActType::Pair, false),
    (ActType::GroupSmall, false),
    (ActType::GroupLarge, false),
];

/// Timeplan start order: placed acts first by their order, then the others
/// by name (the Kür's, then the riders').
fn sort_acts(acts: &mut [ActNode]) {
    acts.sort_by_cached_key(|act| {
        let riders = act
            .participants
            .iter()
            .map(|p| format!("{} {}", p.firstname, p.lastname))
            .collect::<Vec<_>>()
            .join(", ");
        (
            act.order.is_none(),
            act.order,
            act.name.to_lowercase(),
            riders.to_lowercase(),
            act.id,
        )
    });
}

fn count(acts: &[ActNode]) -> i64 {
    acts.iter().filter(|a| !a.withdrawn).count() as i64
}

/// The structure tree, admin-only: discipline, heat, category, act.
///
/// Every category belongs to a heat in a finished plan; the others are in
/// `without_heat` of their discipline. Acts without a category (incomplete
/// groups, withdrawn acts) are in `without_category`. Heats without categories
/// have no discipline and are left out. One consistent read.
#[utoipa::path(
    get,
    tags=["query", "heat"],
    path="/get_structure",
    responses(
        (status=200, content_type="application/json", body=Structure),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn get_structure(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
) -> Result<Json<Structure>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = db.begin().await?;
    let categories: Vec<CategoryRow> = sqlx::query!(
        r#"
        SELECT name as "name!", description, act_type as "act_type: ActType", is_single_male, heat
        FROM categories
        ORDER BY "order", name
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| CategoryRow {
        name: row.name,
        description: row.description,
        discipline: HeatDiscipline::new(row.act_type, row.is_single_male),
        heat: row.heat,
    })
    .collect();
    // The heats in the order of their first category; those without one have
    // no discipline.
    let heats =
        sqlx::query!(r#"SELECT name as "name!", description, skip FROM heats ORDER BY name"#)
            .fetch_all(&mut *tx)
            .await?;
    let mut finals: HashMap<String, (String, i64)> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT heat as "heat!", final_name, COUNT(*) as "slots!: i64"
        FROM final_slots
        GROUP BY heat, final_name
        ORDER BY final_name DESC
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    {
        // A heat feeds at most one final.
        finals.insert(row.heat, (row.final_name, row.slots));
    }
    let acts: Vec<ActRow> = sqlx::query!(
        r#"
        SELECT
            view_act.id as "id!: Uuid",
            view_act."order" as "order",
            view_act.name,
            view_act.group_name,
            view_act.act_type as "act_type!: ActType",
            view_act.category,
            view_act.participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>",
            view_act.withdrawn_at IS NOT NULL as "withdrawn!: bool",
            COALESCE((
                SELECT MIN(s.gender = 'male')
                FROM rider s JOIN act_participants p ON p.rider_id = s.id
                WHERE p.act_id = view_act.id
            ), FALSE) as "male!: bool"
        FROM view_act
        "#
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| ActRow {
        category: row.category,
        discipline: HeatDiscipline::new(row.act_type, row.male),
        node: ActNode {
            id: row.id,
            name: row.name,
            group_name: row.group_name,
            participants: row.participants.0,
            withdrawn: row.withdrawn,
            order: row.order,
        },
    })
    .collect();
    tx.commit().await?;

    let mut acts_of: HashMap<String, Vec<ActNode>> = HashMap::new();
    let mut uncategorized: Vec<(HeatDiscipline, ActNode)> = Vec::new();
    for act in acts {
        match act.category {
            Some(category) => acts_of.entry(category).or_default().push(act.node),
            None => uncategorized.push((act.discipline, act.node)),
        }
    }

    let mut disciplines = Vec::new();
    for (act_type, single_male) in DISCIPLINES {
        let discipline = HeatDiscipline::new(act_type, single_male);
        let mut nodes: HashMap<&str, CategoryNode> = HashMap::new();
        for category in categories.iter().filter(|c| c.discipline == discipline) {
            let mut acts = acts_of.remove(&category.name).unwrap_or_default();
            sort_acts(&mut acts);
            nodes.insert(
                &category.name,
                CategoryNode {
                    name: category.name.clone(),
                    description: category.description.clone(),
                    act_count: count(&acts),
                    acts,
                },
            );
        }
        let mut heat_nodes = Vec::new();
        for heat in &heats {
            let members: Vec<CategoryNode> = categories
                .iter()
                .filter(|c| c.discipline == discipline && c.heat.as_deref() == Some(&heat.name))
                .filter_map(|c| nodes.remove(c.name.as_str()))
                .collect();
            if members.is_empty() {
                continue;
            }
            let (final_name, slots) = match finals.get(&heat.name) {
                Some((name, slots)) => (Some(name.clone()), Some(*slots)),
                None => (None, None),
            };
            heat_nodes.push(HeatNode {
                name: heat.name.clone(),
                description: heat.description.clone(),
                skip: heat.skip,
                final_name,
                slots,
                act_count: members.iter().map(|c| c.act_count).sum(),
                categories: members,
            });
        }
        // By the order of their first category.
        let position = |heat: &HeatNode| {
            categories
                .iter()
                .position(|c| c.name == heat.categories[0].name)
        };
        heat_nodes.sort_by_key(position);
        let without_heat: Vec<CategoryNode> = categories
            .iter()
            .filter_map(|c| nodes.remove(c.name.as_str()))
            .collect();
        let mut without_category: Vec<ActNode> = Vec::new();
        let mut rest = Vec::new();
        for (d, node) in uncategorized {
            if d == discipline {
                without_category.push(node);
            } else {
                rest.push((d, node));
            }
        }
        uncategorized = rest;
        sort_acts(&mut without_category);
        disciplines.push(Discipline {
            act_type,
            is_single_male: single_male,
            act_count: heat_nodes.iter().map(|h| h.act_count).sum::<i64>()
                + without_heat.iter().map(|c| c.act_count).sum::<i64>()
                + count(&without_category),
            heats: heat_nodes,
            without_heat,
            without_category: ActsNode {
                act_count: count(&without_category),
                acts: without_category,
            },
        });
    }
    Ok(Json(Structure { disciplines }))
}
