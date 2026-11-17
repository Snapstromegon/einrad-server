use std::sync::Arc;

use crate::ids::UserId;
use axum::{Extension, Json};
use axum_extra::extract::CookieJar;
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError},
    jwt::JWTConfig,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct VerifyMailResponse {
    user_id: UserId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerifyMailBody {
    token: Uuid,
}

/// VerifyMail a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/verify_email",
    responses(
        (status=200, content_type="application/json", body=VerifyMailResponse),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, jwt_config))]
#[axum::debug_handler]
pub async fn verify_email(
    Extension(db): Extension<DbHandle>,
    Extension(jwt_config): Extension<Arc<JWTConfig>>,
    cookies: CookieJar,
    Json(body): Json<VerifyMailBody>,
) -> Result<(CookieJar, Json<VerifyMailResponse>), HttpError> {
    info!("Verifying email");
    let user_id = db.verify_email(body.token).await?;
    info!("User ID: {:?}", user_id);
    if let Some(user_id) = user_id {
        Ok((
            jwt_config.add_jwt_cookie(cookies, user_id).map_err(|e| {
                tracing::error!("Failed to add JWT cookie: {:?}", e);
                HttpError::InvalidCredentials
            })?,
            Json(VerifyMailResponse { user_id }),
        ))
    } else {
        Err(HttpError::NotFound)
    }
}
