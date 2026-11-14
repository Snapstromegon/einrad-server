use std::sync::Arc;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions},
    mailer::Mailer,
    templates::PasswordResetMail,
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
#[instrument(skip(mailer, db))]
#[axum::debug_handler]
pub async fn request_password_reset(
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Extension(db): Extension<DbHandle>,
    Json(body): Json<RequestPasswordResetBody>,
) -> Result<Json<RequestPasswordResetResponse>, HttpError> {
    let user = db.find_user_by_email(&body.email).await?;
    if let Some(user) = user {
        // Create a token for password reset; the previous ones are dropped
        let reset_token = Uuid::new_v4();
        db.replace_password_reset_token(user.id, reset_token)
            .await?;
        let reset_link = Url::parse(&http_options.base_url)
            .expect("Invalid Base URL")
            .join(&format!("/reset_password?token={}", reset_token))
            .expect("Invalid URL")
            .to_string();
        let event_name = db.event_name().await?;
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
