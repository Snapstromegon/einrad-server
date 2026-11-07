use std::sync::Arc;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    mailer::Mailer,
    reloadable_sqlite::ReloadableSqlite,
    templates::VerifyMail,
};
use askama::Template;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct ResendMailValidationResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResendMailValidationBody {}

/// ResendMailValidation a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/resend_mail_validation",
    request_body=ResendMailValidationBody,
    responses(
        (status=200, content_type="application/json", body=ResendMailValidationResponse),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(mailer))]
#[axum::debug_handler]
pub async fn resend_mail_validation(
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    Json(_body): Json<ResendMailValidationBody>,
) -> Result<Json<ResendMailValidationResponse>, HttpError> {
    let db = db.get().await.clone();
    // Drop the previous verification tokens
    sqlx::query!(
        r#"
        DELETE FROM mail_verification WHERE user_id = ?
        "#,
        auth.user_id
    )
    .execute(&db)
    .await?;
    // Create a token for email verification
    let email_token = Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();
    sqlx::query!(
        r#"
        INSERT INTO mail_verification (id, user_id, created_at)
        VALUES (?, ?, ?)
        "#,
        email_token,
        auth.user_id,
        now
    )
    .execute(&db)
    .await?;

    let verify_link = Url::parse(&http_options.base_url)
        .expect("Invalid Base URL")
        .join(&format!("/verify_email?token={}", email_token))
        .expect("Invalid URL")
        .to_string();
    let event_name = Competition::event_name(&db).await?;
    mailer
        .send_text(
            &auth.email,
            &format!("{event_name} - Email bestätigen"),
            &VerifyMail {
                event_name: &event_name,
                name: &auth.name,
                verify_link: &verify_link,
            }
            .render()?,
        )
        .await
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;

    Ok(Json(ResendMailValidationResponse {}))
}
