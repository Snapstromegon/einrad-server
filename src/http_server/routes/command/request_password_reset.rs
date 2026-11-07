use std::sync::Arc;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, HttpServerOptions},
    mailer::Mailer,
    reloadable_sqlite::ReloadableSqlite,
    templates::PasswordResetMail,
};
use askama::Template;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestPasswordResetResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestPasswordResetBody {
    email: String,
}

/// RequestPasswordReset a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/request_password_reset",
    request_body=RequestPasswordResetBody,
    responses(
        (status=200, content_type="application/json", body=RequestPasswordResetResponse),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(mailer))]
#[axum::debug_handler]
pub async fn request_password_reset(
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Extension(db): Extension<ReloadableSqlite>,
    Json(body): Json<RequestPasswordResetBody>,
) -> Result<Json<RequestPasswordResetResponse>, HttpError> {
    let db = db.get().await.clone();
    let user = sqlx::query!(
        r#"
        SELECT id as "id!: Uuid", name FROM users WHERE email = ?
        "#,
        body.email
    )
    .fetch_optional(&db)
    .await?;
    if let Some(user) = user {
        // Drop the previous verification tokens
        sqlx::query!(
            r#"
            DELETE FROM password_resets WHERE user_id = ?
            "#,
            user.id
        )
        .execute(&db)
        .await?;
        // Create a token for password reset
        let reset_token = Uuid::new_v4();
        let now = time::OffsetDateTime::now_utc();
        sqlx::query!(
            r#"
            INSERT INTO password_resets (id, user_id, created_at)
            VALUES (?, ?, ?)
            "#,
            reset_token,
            user.id,
            now
        )
        .execute(&db)
        .await?;
        let reset_link = Url::parse(&http_options.base_url)
            .expect("Invalid Base URL")
            .join(&format!("/reset_password?token={}", reset_token))
            .expect("Invalid URL")
            .to_string();
        let event_name = Competition::event_name(&db).await?;
        mailer
            .send_text(
                &body.email,
                &format!("{event_name} - Passwort zurücksetzen"),
                &PasswordResetMail {
                    event_name: &event_name,
                    name: &user.name,
                    reset_link: &reset_link,
                }
                .render()?,
            )
            .await
            .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
        Ok(Json(RequestPasswordResetResponse {}))
    } else {
        Err(HttpError::NotFound)
    }
}
