use crate::ids::{ActId, ClubId, RiderId};
use std::collections::HashSet;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    groups::{ensure_club_access, get_group, type_conflict_message},
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetClubGroupMembersResponse {
    /// Set if a rider of the group is now also in another group of the same
    /// type. The change is saved anyway, but the group has no category until
    /// the clubs sort it out. A German message for the user.
    warning: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ClubGroupMemberInput {
    rider_id: RiderId,
    /// Replacement riders (Ersatzfahrer) don't count towards the group size.
    is_replacement: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetClubGroupMembersBody {
    club_id: ClubId,
    act_id: ActId,
    /// All riders of this club in the group. Riders of other clubs are kept.
    /// An empty list means the club leaves the group.
    members: Vec<ClubGroupMemberInput>,
}

/// Set a club's riders in a group.
///
/// Every club can join every group, but only with its own riders. The group's
/// type stays as its owner chose it, whatever the number of riders. If a rider
/// of the group (from any club) is then in two groups of the same type, the
/// change is saved with a warning and both groups lose their category until
/// that's sorted out, since clubs add their riders to a shared group one after
/// another.
#[utoipa::path(
    post,
    tags=["command", "club", "group"],
    path="/set_club_group_members",
    request_body=SetClubGroupMembersBody,
    responses(
        (status=200, content_type="application/json", body=SetClubGroupMembersResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_club_group_members(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetClubGroupMembersBody>,
) -> Result<Json<SetClubGroupMembersResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    ensure_club_access(&mut transaction, &auth, body.club_id).await?;
    let group = get_group(&mut transaction, body.act_id).await?;

    let mut seen = HashSet::new();
    for member in &body.members {
        if !seen.insert(member.rider_id) {
            return Err(HttpError::ErrorMessages(
                "Jede FahrerIn kann nur einmal in der Gruppe angemeldet werden.".to_string(),
            ));
        }
        let club_id = sqlx::query_scalar!(
            r#"SELECT club_id as "club_id!: ClubId" FROM rider WHERE id = ?"#,
            member.rider_id
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if club_id != Some(body.club_id) {
            return Err(HttpError::ErrorMessages(
                "Ein Verein kann nur seine eigenen FahrerInnen zu einer Gruppe hinzufügen."
                    .to_string(),
            ));
        }
    }

    info!(
        "Setting {} riders of club {} in group {:?}",
        body.members.len(),
        body.club_id,
        group.group_name
    );
    sqlx::query!(
        r#"
        DELETE FROM act_participants
        WHERE act_id = ? AND rider_id IN (SELECT id FROM rider WHERE club_id = ?)
        "#,
        group.id,
        body.club_id,
    )
    .execute(&mut *transaction)
    .await?;

    for member in &body.members {
        sqlx::query!(
            r#"
            INSERT INTO act_participants (act_id, rider_id, is_replacement)
            VALUES (?, ?, ?)
            "#,
            group.id,
            member.rider_id,
            member.is_replacement,
        )
        .execute(&mut *transaction)
        .await?;
    }

    let warning = type_conflict_message(&mut transaction, group.id).await?;

    transaction.commit().await?;

    Ok(Json(SetClubGroupMembersResponse { warning }))
}
