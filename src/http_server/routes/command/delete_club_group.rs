use crate::ids::ActId;
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    groups::{ensure_group_owner, get_group},
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::Permission,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteClubGroupResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteClubGroupBody {
    act_id: ActId,
}

/// Delete a group. Its riders stay registered.
///
/// Only the group's owner club (or an admin) can delete it. The owner club
/// can't delete a group while riders of other clubs are still in it.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/delete_club_group",
    request_body=DeleteClubGroupBody,
    responses(
        (status=200, content_type="application/json", body=DeleteClubGroupResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_club_group(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<DeleteClubGroupBody>,
) -> Result<Json<DeleteClubGroupResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    let group = get_group(&mut transaction, body.act_id).await?;
    ensure_group_owner(&mut transaction, &auth, &group).await?;

    if !auth.can(Permission::ManageRegistrations) {
        let other_club_riders = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i64"
            FROM act_participants
            JOIN rider ON rider.id = act_participants.rider_id
            WHERE act_participants.act_id = ? AND rider.club_id IS NOT ?
            "#,
            group.id,
            group.owner_club_id,
        )
        .fetch_one(&mut *transaction)
        .await?;
        if other_club_riders > 0 {
            return Err(HttpError::ErrorMessages(format!(
                "Die Gruppe „{}“ kann nicht gelöscht werden, solange noch FahrerInnen anderer Vereine in ihr angemeldet sind.",
                group.group_name
            )));
        }
    }

    info!("Deleting group {:?} ({})", group.group_name, group.id);
    sqlx::query!("DELETE FROM act_participants WHERE act_id = ?", group.id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query!("DELETE FROM acts WHERE id = ?", group.id)
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;

    Ok(Json(DeleteClubGroupResponse {}))
}
