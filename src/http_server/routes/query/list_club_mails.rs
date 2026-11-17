use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    club_mails::SentClubMail,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
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
#[instrument(skip(repo))]
pub async fn list_club_mails(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::MailClubs>,
) -> Result<Json<Vec<SentClubMail>>, HttpError> {
    Ok(Json(repo.list_club_mails().await?))
}
