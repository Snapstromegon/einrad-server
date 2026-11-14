use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryEnabled, JuryUnitFields},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditJuryUnitResponse {}

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
    _: Granted<permit::ManageJury>,
    Json(body): Json<JuryUnitFields>,
) -> Result<Json<EditJuryUnitResponse>, HttpError> {
    body.validate().map_err(HttpError::ErrorMessages)?;
    info!("Editing jury unit {:?} {}", body.kind, body.id);
    repo.edit_jury_unit(&body).await?;
    Ok(Json(EditJuryUnitResponse {}))
}
