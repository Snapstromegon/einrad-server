use crate::ids::ClubId;
use axum::{Extension, Json};
use db_core::{commands::jury::SetJuryDeploymentsOverride as Credit, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetJuryDeploymentsOverrideResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJuryDeploymentsOverrideBody {
    club_id: ClubId,
    /// The deployments to credit the club with; null to use the registered
    /// ones again.
    deployments: Option<i64>,
}

/// Credit a club with jury deployments instead of the registered ones, e.g.
/// after a late cancellation or as a waiver. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_deployments_override",
    request_body=SetJuryDeploymentsOverrideBody,
    responses(
        (status=200, content_type="application/json", body=SetJuryDeploymentsOverrideResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn set_jury_deployments_override(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManageJury>,
    Json(body): Json<SetJuryDeploymentsOverrideBody>,
) -> Result<Json<SetJuryDeploymentsOverrideResponse>, HttpError> {
    info!(
        "Crediting club {} with {:?} jury deployments",
        body.club_id, body.deployments
    );
    repo.set_jury_deployments_override(
        &granted.actor(),
        Credit {
            club_id: body.club_id,
            deployments: body.deployments,
        },
    )
    .await?;
    Ok(Json(SetJuryDeploymentsOverrideResponse {}))
}
