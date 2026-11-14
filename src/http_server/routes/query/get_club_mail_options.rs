use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    club_mails::{self, Access, ClubMailOptions},
    competition::Features,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// What the club mail form needs: every club with its contacts and what the
/// recipient presets look at, and the variables the sender may use. Money
/// only with club balances, the jury obligation only with the jury or club
/// balances.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/get_club_mail_options",
    responses(
        (status=200, content_type="application/json", body=ClubMailOptions),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn get_club_mail_options(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::MailClubs>,
) -> Result<Json<ClubMailOptions>, HttpError> {
    let features = Features::from_repo(&*repo).await?;
    Ok(Json(
        club_mails::options(&*repo, Access::of(&granted, features)).await?,
    ))
}
