use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::{Role, permit},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetUserRolesResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetUserRolesBody {
    user_id: Uuid,
    /// Every role the user holds afterwards.
    roles: Vec<Role>,
}

/// Grant a user exactly these roles.
///
/// Admin only. Admins can't drop their own Admin role, so there's always
/// one left.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/set_user_roles",
    request_body=SetUserRolesBody,
    responses(
        (status=200, content_type="application/json", body=SetUserRolesResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_user_roles(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Granted<permit::ManageUsers>,
    Json(body): Json<SetUserRolesBody>,
) -> Result<Json<SetUserRolesResponse>, HttpError> {
    let keeps_admin = body.roles.contains(&Role::Admin);
    if auth.user_id == body.user_id && !keeps_admin {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;

    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id = ?")
        .bind(body.user_id)
        .fetch_optional(&mut *transaction)
        .await?;
    exists.ok_or(HttpError::NotFound)?;

    sqlx::query("DELETE FROM user_roles WHERE user_id = ?")
        .bind(body.user_id)
        .execute(&mut *transaction)
        .await?;
    for role in &body.roles {
        sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role) VALUES (?, ?)")
            .bind(body.user_id)
            .bind(role)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    info!("Set the roles of user {} to {:?}", body.user_id, body.roles);
    Ok(Json(SetUserRolesResponse {}))
}
