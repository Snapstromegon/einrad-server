use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    groups::{ensure_group_owner, ensure_group_type, get_group, type_conflict_message},
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetClubGroupTypeResponse {
    /// Set if a rider of the group is now also in another group of the new
    /// type. The change is saved anyway, but the groups have no category until
    /// the clubs sort it out. A German message for the user.
    warning: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetClubGroupTypeBody {
    act_id: Uuid,
    /// `group_small` (Kleingruppe) or `group_large` (Großgruppe).
    act_type: ActType,
}

/// Change a group's type (Kleingruppe or Großgruppe).
///
/// Only the group's owner club (or an admin) can change it. The group then
/// belongs to the other type's categories.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/set_club_group_type",
    request_body=SetClubGroupTypeBody,
    responses(
        (status=200, content_type="application/json", body=SetClubGroupTypeResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_club_group_type(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetClubGroupTypeBody>,
) -> Result<Json<SetClubGroupTypeResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    let group = get_group(&mut transaction, body.act_id).await?;
    ensure_group_owner(&mut transaction, &auth, &group).await?;
    ensure_group_type(body.act_type)?;

    if group.act_type != body.act_type {
        info!(
            "Changing group {:?} from {} to {}",
            group.group_name,
            group.act_type.as_str(),
            body.act_type.as_str()
        );
        sqlx::query!(
            "UPDATE acts SET act_type = ? WHERE id = ?",
            body.act_type,
            group.id,
        )
        .execute(&mut *transaction)
        .await?;
    }
    let warning = type_conflict_message(&mut transaction, group.id).await?;

    transaction.commit().await?;

    Ok(Json(SetClubGroupTypeResponse { warning }))
}
