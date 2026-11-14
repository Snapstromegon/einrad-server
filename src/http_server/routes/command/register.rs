use std::sync::Arc;

use crate::ids::UserId;
use askama::Template;
use axum::{Extension, Json, http::StatusCode};
use axum_extra::extract::CookieJar;
use db_core::{DbHandle, auth::NewUser};
use password_auth::generate_hash;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions},
    jwt::JWTConfig,
    mailer::Mailer,
    system_status::Capabilities,
    templates::VerifyMail,
    utils::check_password,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    user_id: UserId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterBody {
    name: String,
    password: String,
    email: String,
}

/// Register a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "user"],
    path="/register",
    request_body=RegisterBody,
    responses(
        (status=200, content_type="application/json", body=RegisterResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, mailer))]
#[axum::debug_handler]
pub async fn register(
    cookies: CookieJar,
    capabilities: Capabilities,
    Extension(db): Extension<DbHandle>,
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(jwt_config): Extension<Arc<JWTConfig>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Json(body): Json<RegisterBody>,
) -> Result<(CookieJar, Json<RegisterResponse>), HttpError> {
    if !capabilities.can_register {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    check_password(&body.password).map_err(HttpError::ErrorMessages)?;
    if db.email_in_use(&body.email).await? {
        return Err(HttpError::ErrorMessages(
            "Email Adresse bereits registriert.".to_string(),
        ));
    }
    let user_id = UserId::new();
    let hashed_password = generate_hash(body.password);
    // Create a token for email verification
    let email_token = Uuid::new_v4();
    db.register_user(
        NewUser {
            id: user_id,
            name: &body.name,
            email: &body.email,
            password_hash: &hashed_password,
        },
        email_token,
    )
    .await?;

    let verify_link = Url::parse(&http_options.base_url)
        .expect("Invalid Base URL")
        .join(&format!("/verify_email?token={}", email_token))
        .expect("Invalid URL")
        .to_string();

    let event_name = db.event_name().await?;
    mailer
        .send_text(
            &body.email,
            &format!("{event_name} - Email bestätigen"),
            &VerifyMail {
                event_name: &event_name,
                name: &body.name,
                verify_link: &verify_link,
            }
            .render()?,
        )
        .await?;

    Ok((
        jwt_config.add_jwt_cookie(cookies, user_id).map_err(|e| {
            tracing::error!("Failed to add JWT cookie: {:?}", e);
            HttpError::InvalidCredentials
        })?,
        Json(RegisterResponse { user_id }),
    ))
}
