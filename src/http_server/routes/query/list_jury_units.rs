use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JuryEnabled, JuryUnit},
    reloadable_sqlite::ReloadableSqlite,
};

/// List the jury units judges register for: the Vorlauf of every heat and
/// every final, in the order of the timeplan, with the day they run on. For
/// logged-in users.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_jury_units",
    responses(
        (status=200, content_type="application/json", body=Vec<JuryUnit>),
        (status=401, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_jury_units(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _auth: Auth,
) -> Result<Json<Vec<JuryUnit>>, HttpError> {
    let db = db.get().await.clone();
    Ok(Json(jury::list_units(&db).await?))
}
