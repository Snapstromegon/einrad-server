use crate::ids::UserId;
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::{Role, permit},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetUserRolesResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetUserRolesBody {
    user_id: UserId,
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
    Extension(db): Extension<DbHandle>,
    auth: Granted<permit::ManageUsers>,
    Json(body): Json<SetUserRolesBody>,
) -> Result<Json<SetUserRolesResponse>, HttpError> {
    let keeps_admin = body.roles.contains(&Role::Admin);
    if auth.user_id == body.user_id && !keeps_admin {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    if !db.set_roles(body.user_id, &body.roles).await? {
        return Err(HttpError::NotFound);
    }
    info!("Set the roles of user {} to {:?}", body.user_id, body.roles);
    Ok(Json(SetUserRolesResponse {}))
}
