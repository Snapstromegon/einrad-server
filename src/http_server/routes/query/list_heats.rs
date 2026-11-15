use crate::ids::{CategoryId, FinalId, HeatId};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::heat as read};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError},
};

/// A final a heat fills slots of, as the heat list shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct HeatFinal {
    pub id: FinalId,
    /// The readable name.
    pub name: String,
    /// The short code.
    pub code: String,
}

/// A heat as the API lists it.
#[derive(Debug, Serialize, ToSchema)]
pub struct Heat {
    pub id: HeatId,
    /// The readable name, unique, e.g. "Einzel männlich – Expert 1 (U17–U21)".
    pub name: String,
    /// The short code, unique, e.g. "E EM 1".
    pub code: String,
    pub description: Option<String>,
    /// Its categories, in category order. Empty until the admin assigns
    /// them.
    pub categories: Vec<String>,
    /// The same categories by id, in the same order.
    pub category_ids: Vec<CategoryId>,
    /// The act type its categories share; `None` without categories.
    pub act_type: Option<ActType>,
    /// Whether its categories are singles male; `None` without categories.
    pub is_single_male: Option<bool>,
    /// The finals whose slots it fills; at most one.
    pub finals: Vec<HeatFinal>,
    /// The fewest acts it needs for its Vorlauf.
    pub threshold: i64,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    pub skip: bool,
    /// Its acts at the moment, without withdrawn ones.
    pub act_count: i64,
    /// Whether the tool suggests skipping its Vorlauf: it has fewer acts than
    /// its threshold; never for groups, which start in their final. The
    /// organizer decides.
    pub skip_suggested: bool,
}

impl From<read::HeatFinal> for HeatFinal {
    fn from(heat_final: read::HeatFinal) -> Self {
        Self {
            id: heat_final.id,
            name: heat_final.name,
            code: heat_final.code,
        }
    }
}

impl From<read::Heat> for Heat {
    fn from(heat: read::Heat) -> Self {
        Self {
            id: heat.id,
            name: heat.name,
            code: heat.code,
            description: heat.description,
            categories: heat.categories,
            category_ids: heat.category_ids,
            act_type: heat.act_type,
            is_single_male: heat.is_single_male,
            finals: heat.finals.into_iter().map(HeatFinal::from).collect(),
            threshold: heat.threshold,
            skip: heat.skip,
            act_count: heat.act_count,
            skip_suggested: heat.skip_suggested,
        }
    }
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
#[instrument(skip(repo))]
pub async fn list_heats(
    Extension(repo): Extension<DbHandle>,
) -> Result<Json<Vec<Heat>>, HttpError> {
    Ok(Json(
        repo.list_heats()
            .await?
            .into_iter()
            .map(Heat::from)
            .collect(),
    ))
}
