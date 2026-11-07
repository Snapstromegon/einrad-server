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
    if user_manager.is_none() && query.token != Some(http_options.reload_db_token.clone()) {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }

    db.reload().await?;

    Ok(Json(ReloadDBResponse {}))
}
