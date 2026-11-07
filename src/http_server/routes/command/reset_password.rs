use crate::{
    http_server::{ClientError, HttpError},
    reloadable_sqlite::ReloadableSqlite,
    utils::check_password,
};
use axum::{Extension, Json};
use password_auth::generate_hash;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct PasswordResetResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordResetBody {
    token: Uuid,
    new_password: String,
}

/// PasswordReset a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/reset_password",
    request_body=PasswordResetBody,
    responses(
        (status=200, content_type="application/json", body=PasswordResetResponse),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn reset_password(
    Extension(db): Extension<ReloadableSqlite>,
    Json(body): Json<PasswordResetBody>,
) -> Result<Json<PasswordResetResponse>, HttpError> {
    let db = db.get().await.clone();
    let user = sqlx::query!(
        r#"
        SELECT user_id as "user_id!: Uuid" FROM password_resets WHERE id = ?
        "#,
        body.token
    )
    .fetch_optional(&db)
    .await?;
    if let Some(user) = user {
        check_password(&body.new_password).map_err(HttpError::ErrorMessages)?;
        // Update the password
        let password_hash = generate_hash(body.new_password);
        sqlx::query!(
            r#"
            UPDATE users SET password = ? WHERE id = ?
            "#,
            password_hash,
            user.user_id
        )
        .execute(&db)
        .await?;
        // Drop the previous verification tokens
        sqlx::query!(
            r#"
            DELETE FROM password_resets WHERE user_id = ?
            "#,
            user.user_id
        )
        .execute(&db)
        .await?;
        Ok(Json(PasswordResetResponse {}))
    } else {
        Err(HttpError::NotFound)
    }
}
