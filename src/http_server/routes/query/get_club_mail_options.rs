use std::sync::Arc;

use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    club_mails::{self, ClubMailOptions, Sender},
    competition::Features,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
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
#[instrument(skip(db, http_options))]
pub async fn get_club_mail_options(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    granted: Granted<permit::MailClubs>,
) -> Result<Json<ClubMailOptions>, HttpError> {
    let db = db.get().await.clone();
    let features = Features::current(&db, &http_options.features).await?;
    Ok(Json(
        club_mails::options(&db, Sender::of(&granted), features.jury_registration).await?,
    ))
}
