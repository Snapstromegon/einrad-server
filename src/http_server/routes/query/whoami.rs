use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::http_server::{
    HttpError,
    extractor::auth::Auth,
    routes::http_types::{CurrentUser, User},
};

/// Get information about the currently authenticated user.
#[utoipa::path(
    get,
    tags=["query", "auth"],
    path="/whoami",
    responses(
        (status=200, content_type="application/json", body=CurrentUser),
    ),
)]
#[instrument(skip(db))]
pub async fn whoami(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
) -> Result<Json<CurrentUser>, HttpError> {
    let user = db
        .find_user(auth.user_id)
        .await?
        .ok_or(HttpError::NotFound)?;
    Ok(Json(CurrentUser {
        user: User {
            roles: auth.roles.clone(),
            ..user
        },
        permissions: auth.permissions(),
    }))
}
