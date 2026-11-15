use crate::ids::JuryUnitId;
use axum::{Extension, Json};
use db_core::{commands::jury::EditJuryUnit, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{
        ClientError, HttpError, extractor::auth::Granted, routes::http_types::JuryUnitKind,
    },
    jury::JuryEnabled,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditJuryUnitResponse {}

/// What the jury lead sets for a jury unit (JURY-2, D2).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct JuryUnitFields {
    pub kind: JuryUnitKind,
    pub id: JuryUnitId,
    pub registrable: bool,
    pub deployments: i64,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
}

/// Change the jury settings of a heat's Vorlauf or of a final: whether judges
/// can register for it, its deployments and the judges it needs per task.
/// Changing its deployments changes every club's registered deployments.
/// Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/edit_jury_unit",
    request_body=JuryUnitFields,
    responses(
        (status=200, content_type="application/json", body=EditJuryUnitResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_jury_unit(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManageJury>,
    Json(body): Json<JuryUnitFields>,
) -> Result<Json<EditJuryUnitResponse>, HttpError> {
    info!("Editing jury unit {:?} {}", body.kind, body.id);
    repo.edit_jury_unit(
        &granted.actor(),
        EditJuryUnit {
            fields: db_core::jury::JuryUnitFields {
                kind: body.kind.into(),
                id: body.id,
                registrable: body.registrable,
                deployments: body.deployments,
                needed_technical: body.needed_technical,
                needed_performance: body.needed_performance,
                needed_dismounts: body.needed_dismounts,
            },
        },
    )
    .await?;
    Ok(Json(EditJuryUnitResponse {}))
}
