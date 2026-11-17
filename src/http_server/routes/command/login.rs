use crate::ids::UserId;
use std::sync::Arc;

use axum::{Extension, Json};
use axum_extra::extract::CookieJar;
use db_core::DbHandle;
use password_auth::verify_password;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{http_server::HttpError, jwt::JWTConfig};

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    user_id: UserId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginBody {
    password: String,
    email: String,
}

/// Login a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/login",
    request_body=LoginBody,
    responses(
        (status=200, content_type="application/json", body=LoginResponse),
        (status=401, content_type="application/json", body=String),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn login(
    Extension(db): Extension<DbHandle>,
    cookies: CookieJar,
    Extension(jwt_config): Extension<Arc<JWTConfig>>,
    Json(body): Json<LoginBody>,
) -> Result<(CookieJar, Json<LoginResponse>), HttpError> {
    let jwt_config = jwt_config.as_ref().clone();
    let user = db
        .find_credentials(&body.email)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {:?}", e);
            HttpError::InvalidCredentials
        })?
        .ok_or(HttpError::InvalidCredentials)?;

    if verify_password(body.password, &user.password_hash).is_ok() {
        Ok((
            jwt_config.add_jwt_cookie(cookies, user.id).map_err(|e| {
                tracing::error!("Failed to add JWT cookie: {:?}", e);
                HttpError::InvalidCredentials
            })?,
            Json(LoginResponse { user_id: user.id }),
        ))
    } else {
        Err(HttpError::InvalidCredentials)
    }
}
