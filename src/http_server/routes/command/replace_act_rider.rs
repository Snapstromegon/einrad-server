use crate::ids::{ActId, RiderId};
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration,
    roles::Permission,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ReplaceActRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReplaceActRiderBody {
    act_id: ActId,
    /// The rider leaving the act.
    rider_id: RiderId,
    /// The rider taking their place, of the same club.
    new_rider_id: RiderId,
}

/// Replace a rider of a single or pair, keeping the act with its title,
/// description and music.
///
/// For a single, the new rider gets the single (by their gender, which must
/// be known) and the old one loses it. For a pair, the new rider becomes the
/// partner of the remaining rider (and their pair request is fulfilled). The
/// new rider must be of the same club (admins: any club) and not already ride
/// a single or pair. Group riders
/// are set with `set_club_group_members`.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/replace_act_rider",
    request_body=ReplaceActRiderBody,
    responses(
        (status=200, content_type="application/json", body=ReplaceActRiderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn replace_act_rider(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<ReplaceActRiderBody>,
) -> Result<Json<ReplaceActRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    let withdrawn = sqlx::query_scalar!(
        r#"SELECT withdrawn_at IS NOT NULL as "withdrawn!: bool" FROM acts WHERE id = $1"#,
        body.act_id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if withdrawn {
        return Err(HttpError::ErrorMessages(
            "Die Kür ist abgemeldet und kann nicht mehr geändert werden.".to_string(),
        ));
    }

    let club_id = ensure_rider_access(&mut transaction, &auth, body.rider_id).await?;
    let new_club_id = ensure_rider_access(&mut transaction, &auth, body.new_rider_id).await?;
    if club_id != new_club_id && !auth.can(Permission::ManageRegistrations) {
        return Err(HttpError::ErrorMessages(
            "Der neue Fahrer muss aus demselben Verein sein.".to_string(),
        ));
    }

    let participants = sqlx::query_scalar!(
        r#"SELECT rider_id as "rider_id!: RiderId" FROM act_participants WHERE act_id = $1"#,
        body.act_id,
    )
    .fetch_all(&mut *transaction)
    .await?;
    if !participants.contains(&body.rider_id) {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    if participants.contains(&body.new_rider_id) {
        return Err(HttpError::ErrorMessages(
            "Der Fahrer fährt diese Kür schon.".to_string(),
        ));
    }

    rider_registration::replace_rider(
        &mut transaction,
        body.act_id,
        body.rider_id,
        body.new_rider_id,
    )
    .await?;

    transaction.commit().await?;
    Ok(Json(ReplaceActRiderResponse {}))
}
