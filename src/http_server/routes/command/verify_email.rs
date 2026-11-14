use std::sync::Arc;

use crate::ids::UserId;
use axum::{Extension, Json};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError},
    jwt::JWTConfig,
    reloadable_sqlite::ReloadableSqlite,
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
    Extension(db): Extension<ReloadableSqlite>,
    Extension(jwt_config): Extension<Arc<JWTConfig>>,
    cookies: CookieJar,
    Json(body): Json<VerifyMailBody>,
) -> Result<(CookieJar, Json<VerifyMailResponse>), HttpError> {
    info!("Verifying email");
    let db = db.get().await.clone();
    let user = sqlx::query!(
        r#"
        SELECT user_id as "user_id!: UserId" FROM mail_verification WHERE id = ?
        "#,
        body.token
    )
    .fetch_optional(&db)
    .await?;
    info!("User ID: {:?}", user);
    if let Some(user) = user {
        sqlx::query!(
            r#"UPDATE users SET email_verified = true WHERE id = ?"#,
            user.user_id
        )
        .execute(&db)
        .await?;
        sqlx::query!(
            r#"DELETE FROM mail_verification WHERE user_id = ?"#,
            user.user_id
        )
        .execute(&db)
        .await?;
        info!("Found user: {:?}", user);
        Ok((
            jwt_config
                .add_jwt_cookie(cookies, user.user_id)
                .map_err(|e| {
                    tracing::error!("Failed to add JWT cookie: {:?}", e);
                    HttpError::InvalidCredentials
                })?,
            Json(VerifyMailResponse {
                user_id: user.user_id,
            }),
        ))
    } else {
        Err(HttpError::NotFound)
    }
}
