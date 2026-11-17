use crate::ids::JuryUnitId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryEnabled, JuryUnitKind},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ResolveUnmappedRegistrationResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveUnmappedRegistrationBody {
    /// The id of the unmapped registration.
    id: i64,
    /// The jury unit to register the judge for instead; `None` drops the
    /// registration.
    unit: Option<UnitRef>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnitRef {
    kind: JuryUnitKind,
    id: JuryUnitId,
}

/// Resolve a registration the migration to jury units could not map (JURY-5):
/// register the judge for a heat's Vorlauf or a final, or drop it. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/resolve_unmapped_registration",
    request_body=ResolveUnmappedRegistrationBody,
    responses(
        (status=200, content_type="application/json", body=ResolveUnmappedRegistrationResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn resolve_unmapped_registration(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<ResolveUnmappedRegistrationBody>,
) -> Result<Json<ResolveUnmappedRegistrationResponse>, HttpError> {
    info!("Resolving unmapped registration {}", body.id);
    repo.resolve_unmapped_registration(body.id, body.unit.map(|unit| (unit.kind, unit.id)))
        .await?;
    Ok(Json(ResolveUnmappedRegistrationResponse {}))
}
