use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError,
        extractor::{
            auth::Granted,
            startlist::{ActListAccess, StartlistAccess},
        },
        routes::http_types::Act,
    },
    roles::permit,
};

/// List all acts.
///
/// Before the start list is published only logged-in users get it (403 for
/// visitors).
///
/// Sorted by category and act order. Acts without a category (e.g. groups
/// with fewer than 3 riders) come last. `max_age` is only filled for admins.
#[utoipa::path(
    get,
    tags=["query", "acts"],
    path="/list_acts",
    responses(
        (status=200, content_type="application/json", body=Vec<Act>),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_acts(
    _: ActListAccess,
    Extension(db): Extension<DbHandle>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
    payments: Option<Granted<permit::ManagePayments>>,
    startlist: Option<StartlistAccess>,
) -> Result<Json<Vec<Act>>, HttpError> {
    let acts = db.list_acts().await?;
    Ok(Json(
        acts.into_iter()
            .map(|act| {
                Act::from(act).for_viewer(
                    birthdates.is_some(),
                    payments.is_some(),
                    startlist.is_some(),
                )
            })
            .collect(),
    ))
}
