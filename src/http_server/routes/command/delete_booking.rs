use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    bookings::{self, Actor, Booking, BookingAction},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteBookingResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBookingBody {
    booking_id: Uuid,
}

/// Delete a booking, e.g. one recorded twice; the payment log keeps it.
/// Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/delete_booking",
    request_body=DeleteBookingBody,
    responses(
        (status=200, content_type="application/json", body=DeleteBookingResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_booking(
    Extension(db): Extension<ReloadableSqlite>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<DeleteBookingBody>,
) -> Result<Json<DeleteBookingResponse>, HttpError> {
    let id = body.booking_id;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let before = Booking::load(&mut tx, id)
        .await?
        .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    info!("Deleting booking {id}");
    sqlx::query!("DELETE FROM bookings WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    bookings::log_change(
        &mut tx,
        id,
        &Actor::of(&granted),
        BookingAction::Deleted,
        Some(&before),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(DeleteBookingResponse {}))
}
