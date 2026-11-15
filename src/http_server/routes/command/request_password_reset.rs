use std::sync::Arc;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions},
    mailer::Mailer,
    templates::PasswordResetMail,
};
use askama::Template;
use axum::{Extension, Json};
use db_core::{commands::user::RequestPasswordReset, error::CommandError, prelude::*};
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
    // Create a token for password reset; the command drops the previous ones
    let reset_token = Uuid::new_v4();
    let user = match db
        .request_password_reset(RequestPasswordReset {
            email: body.email.clone(),
            token: reset_token,
        })
        .await
    {
        Ok(user) => Some(user),
        Err(CommandError::NotFound) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(user) = user {
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
