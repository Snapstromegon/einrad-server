use std::sync::Arc;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    mailer::Mailer,
    templates::VerifyMail,
};
use askama::Template;
use axum::{Extension, Json};
use db_core::DbHandle;
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
#[instrument(skip(mailer, db))]
#[axum::debug_handler]
pub async fn resend_mail_validation(
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    Json(_body): Json<ResendMailValidationBody>,
) -> Result<Json<ResendMailValidationResponse>, HttpError> {
    // Create a token for email verification; the previous ones are dropped
    let email_token = Uuid::new_v4();
    db.replace_verification_token(auth.user_id, email_token)
        .await?;

    let verify_link = Url::parse(&http_options.base_url)
        .expect("Invalid Base URL")
        .join(&format!("/verify_email?token={}", email_token))
        .expect("Invalid URL")
        .to_string();
    let event_name = db.event_name().await?;
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
