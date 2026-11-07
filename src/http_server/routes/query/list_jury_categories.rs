use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JuryCategory, JuryEnabled},
    reloadable_sqlite::ReloadableSqlite,
};

/// List the jury categories judges register for, in order. For logged-in
/// users.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_jury_categories",
    responses(
        (status=200, content_type="application/json", body=Vec<JuryCategory>),
        (status=401, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_jury_categories(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _auth: Auth,
) -> Result<Json<Vec<JuryCategory>>, HttpError> {
    let db = db.get().await.clone();
    Ok(Json(jury::list_categories(&db).await?))
}
