use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    bookings::{self, BookingLogEntry},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListBookingLogQuery {
    /// Only changes to payments of this club, before or after the change.
    club_id: Option<Uuid>,
}

/// The payment log: every change to a booking with the values before and
/// after, newest first. Treasurer.
#[utoipa::path(
    get,
    tags=["query", "payment"],
    path="/list_booking_log",
    params(ListBookingLogQuery),
    responses(
        (status=200, content_type="application/json", body=Vec<BookingLogEntry>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_booking_log(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManagePayments>,
    Query(query): Query<ListBookingLogQuery>,
) -> Result<Json<Vec<BookingLogEntry>>, HttpError> {
    let db = db.get().await.clone();
    let mut log = bookings::log(&db).await?;
    if let Some(club_id) = query.club_id {
        log.retain(|entry| entry.concerns_club(club_id));
    }
    Ok(Json(log))
}
