use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration::{self, PartnerClubs},
    roles::Permission,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct LinkPairResponse {
    act_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkPairBody {
    rider_id: Uuid,
    partner_id: Uuid,
}

/// Links two riders to a pair act; their pair requests are fulfilled.
///
/// Clubs link riders of their own club, the organizer also riders of
/// different clubs. Fails if one of them already rides a pair.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/link_pair",
    request_body=LinkPairBody,
    responses(
        (status=200, content_type="application/json", body=LinkPairResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn link_pair(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<LinkPairBody>,
) -> Result<Json<LinkPairResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_rider_access(&mut transaction, &auth, body.rider_id).await?;
    let clubs = if auth.can(Permission::ManageRegistrations) {
        PartnerClubs::Any
    } else {
        PartnerClubs::Same
    };
    let act_id =
        rider_registration::link_pair(&mut transaction, body.rider_id, body.partner_id, clubs)
            .await?;
    transaction.commit().await?;
    Ok(Json(LinkPairResponse { act_id }))
}
