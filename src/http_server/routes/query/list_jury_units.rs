use crate::act_type::ActType;
use crate::ids::{FinalId, JuryUnitId};
use axum::{Extension, Json};
use db_core::prelude::*;
use time::Date;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError, extractor::auth::Auth, routes::http_types::JuryUnitKind,
    },
    jury::JuryEnabled,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

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
}

impl From<db_core::queries::jury::JuryUnit> for JuryUnit {
    fn from(unit: db_core::queries::jury::JuryUnit) -> Self {
        Self {
            id: unit.id,
            kind: unit.kind.into(),
            name: unit.name,
            code: unit.code,
            description: unit.description,
            registrable: unit.registrable,
            registrable_setting: unit.registrable_setting,
            skipped: unit.skipped,
            deployments: unit.deployments,
            needed_technical: unit.needed_technical,
            needed_performance: unit.needed_performance,
            needed_dismounts: unit.needed_dismounts,
            act_type: unit.act_type,
            final_id: unit.final_id,
            day: unit.day,
        }
    }
}

/// List the jury units judges register for: the Vorlauf of every heat and
/// every final, in the order of the timeplan, with the day they run on. For
/// logged-in users.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_jury_units",
    responses(
        (status=200, content_type="application/json", body=Vec<JuryUnit>),
        (status=401, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_jury_units(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    auth: Auth,
) -> Result<Json<Vec<JuryUnit>>, HttpError> {
    let units = repo.list_jury_units(&auth.actor()).await?;
    Ok(Json(units.into_iter().map(Into::into).collect()))
}
