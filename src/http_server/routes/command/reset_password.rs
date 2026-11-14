use crate::{
    http_server::{ClientError, HttpError},
    utils::check_password,
};
use axum::{Extension, Json};
use db_core::DbHandle;
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
    Extension(db): Extension<DbHandle>,
    Json(body): Json<PasswordResetBody>,
) -> Result<Json<PasswordResetResponse>, HttpError> {
    let user_id = db.find_password_reset(body.token).await?;
    if let Some(user_id) = user_id {
        check_password(&body.new_password).map_err(HttpError::ErrorMessages)?;
        // Update the password; the reset tokens go with it
        let password_hash = generate_hash(body.new_password);
        db.set_password(user_id, &password_hash).await?;
        Ok(Json(PasswordResetResponse {}))
    } else {
        Err(HttpError::NotFound)
    }
}
