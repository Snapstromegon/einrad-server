use axum::{Extension, Json};
use db_core::prelude::*;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::http_types::User},
    roles::permit,
};

/// List all users.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/list_users",
    responses(
        (status=200, content_type="application/json", body=Vec<User>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_users(
    Extension(db): Extension<DbHandle>,
    auth: Granted<permit::ManageUsers>,
) -> Result<Json<Vec<User>>, HttpError> {
    let users = db.list_users(&auth.actor()).await?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}
