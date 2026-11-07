use std::sync::Arc;

use askama::Template;
use axum::{Extension, Json, http::StatusCode};
use axum_extra::extract::CookieJar;
use password_auth::generate_hash;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::instrument;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, HttpServerOptions},
    jwt::JWTConfig,
    mailer::Mailer,
    reloadable_sqlite::ReloadableSqlite,
    system_status::Capabilities,
    templates::VerifyMail,
    utils::check_password,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    user_id: Uuid,
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
    Extension(db): Extension<ReloadableSqlite>,
    Extension(mailer): Extension<Arc<Mailer>>,
    Extension(jwt_config): Extension<Arc<JWTConfig>>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Json(body): Json<RegisterBody>,
) -> Result<(CookieJar, Json<RegisterResponse>), HttpError> {
    if !capabilities.can_register {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    check_password(&body.password).map_err(HttpError::ErrorMessages)?;
    check_email_exists(&db, &body.email).await?;
    let user_id = Uuid::now_v7();
    let hashed_password = generate_hash(body.password);
    sqlx::query!(
        r#"
        INSERT INTO users (id, name, email, email_verified, password)
        VALUES (?, ?, ?, ?, ?)
        "#,
        user_id,
        body.name,
        body.email,
        false,
        hashed_password,
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
        user_id,
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

async fn check_email_exists(db: &SqlitePool, email: &str) -> Result<(), HttpError> {
    let user = sqlx::query!(
        r#"
        SELECT id FROM users WHERE email = ?
        "#,
        email
    )
    .fetch_optional(db)
    .await?;

    if user.is_some() {
        return Err(HttpError::ErrorMessages(
            "Email Adresse bereits registriert.".to_string(),
        ));
    }

    Ok(())
}
