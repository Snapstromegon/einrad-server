use crate::ids::ActId;
use axum::{Extension, Json, extract::Query};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError,
        extractor::{auth::Granted, startlist::StartlistAccess},
        routes::http_types::Act,
    },
    roles::permit,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetActQuery {
    act_id: ActId,
}

/// Get information about an act.
///
/// `max_age` is only filled for admins.
#[utoipa::path(
    get,
    tags=["query", "act"],
    path="/get_act",
    params(GetActQuery),
    responses(
        (status=200, content_type="application/json", body=Act),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn get_act(
    Extension(db): Extension<DbHandle>,
    Query(query): Query<GetActQuery>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
    payments: Option<Granted<permit::ManagePayments>>,
    startlist: Option<StartlistAccess>,
) -> Result<Json<Act>, HttpError> {
    let act = db
        .find_act(query.act_id)
        .await?
        .ok_or(HttpError::NotFound)?;
    Ok(Json(Act::from(act).for_viewer(
        birthdates.is_some(),
        payments.is_some(),
        startlist.is_some(),
    )))
}
