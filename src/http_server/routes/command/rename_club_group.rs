use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::{ensure_group_name_is_free, ensure_group_owner, get_group, normalize_group_name},
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RenameClubGroupResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameClubGroupBody {
    act_id: Uuid,
    group_name: String,
}

/// Rename a group.
///
/// Only the group's owner club (or an admin) can rename it.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/rename_club_group",
    request_body=RenameClubGroupBody,
    responses(
        (status=200, content_type="application/json", body=RenameClubGroupResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn rename_club_group(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<RenameClubGroupBody>,
) -> Result<Json<RenameClubGroupResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    let group = get_group(&mut transaction, body.act_id).await?;
    ensure_group_owner(&mut transaction, &auth, &group).await?;
    let group_name = normalize_group_name(&body.group_name).map_err(HttpError::ErrorMessages)?;
    ensure_group_name_is_free(&mut transaction, &group_name, Some(group.id)).await?;

    info!("Renaming group {:?} to {group_name:?}", group.group_name);
    sqlx::query!(
        "UPDATE acts SET group_name = ? WHERE id = ?",
        group_name,
        group.id,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(Json(RenameClubGroupResponse {}))
}
