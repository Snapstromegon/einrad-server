use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    club_mails::{self, SentClubMail},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// The club mails sent, newest first, each as written and as every
/// recipient got it. Everyone who sends club mails sees all of them.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/list_club_mails",
    responses(
        (status=200, content_type="application/json", body=Vec<SentClubMail>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_mails(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::MailClubs>,
) -> Result<Json<Vec<SentClubMail>>, HttpError> {
    let db = db.get().await.clone();
    Ok(Json(club_mails::log(&db).await?))
}
