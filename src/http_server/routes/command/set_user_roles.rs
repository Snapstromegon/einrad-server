use crate::ids::UserId;
use axum::{Extension, Json};
use db_core::{commands::user::SetUserRoles, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::http_types::Role},
    roles::permit,
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
    let roles: Vec<db_core::roles::Role> = body.roles.into_iter().map(Into::into).collect();
    db.set_user_roles(
        &auth.actor(),
        SetUserRoles {
            user_id: body.user_id,
            roles: roles.clone(),
        },
    )
    .await?;
    info!("Set the roles of user {} to {:?}", body.user_id, roles);
    Ok(Json(SetUserRolesResponse {}))
}
