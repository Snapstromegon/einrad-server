use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{
        ClientError, HttpError, HttpServerOptions,
        extractor::auth::{Auth, Granted},
    },
    metrics::constant_time_eq,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ReloadDBResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReloadDBQuery {
    pub token: Option<String>,
}

#[derive(ToSchema, Debug)]
pub struct Upload {}

/// Reload the database from disk, e.g. after `seed_demo` replaced it.
///
/// Needs a login and the permission to manage users or the reload token.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/reload_db",
    request_body=Upload,
    responses(
        (status=200, content_type="application/json", body=ReloadDBResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn reload_db(
    Extension(db): Extension<ReloadableSqlite>,
    // A login is needed even with the token.
    _: Auth,
    user_manager: Option<Granted<permit::ManageUsers>>,
    Query(query): Query<ReloadDBQuery>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
) -> Result<Json<ReloadDBResponse>, HttpError> {
    // Without a configured token, only admins can reload.
    let token_matches = http_options.reload_db_token.as_ref().is_some_and(|token| {
        query
            .token
            .as_deref()
            .is_some_and(|given| constant_time_eq(given.as_bytes(), token.as_bytes()))
    });
    if user_manager.is_none() && !token_matches {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }

    db.reload().await?;

    Ok(Json(ReloadDBResponse {}))
}
