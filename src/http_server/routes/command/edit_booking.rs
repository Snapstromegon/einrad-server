use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    bookings::{self, Actor, Booking, BookingAction, BookingInput},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditBookingResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditBookingBody {
    booking_id: Uuid,
    booking: BookingInput,
}

/// Change a booking: its amount, date or note, or assign it to a club,
/// unassign or ignore it. Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/edit_booking",
    request_body=EditBookingBody,
    responses(
        (status=200, content_type="application/json", body=EditBookingResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_booking(
    Extension(db): Extension<ReloadableSqlite>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<EditBookingBody>,
) -> Result<Json<EditBookingResponse>, HttpError> {
    let input = body
        .booking
        .checked()
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let id = body.booking_id;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let before = Booking::load(&mut tx, id)
        .await?
        .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if let Some(club_id) = input.club_id
        && sqlx::query_scalar!("SELECT 1 as one FROM clubs WHERE id = ?", club_id)
            .fetch_optional(&mut *tx)
            .await?
            .is_none()
    {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    info!("Editing booking {id}");
    sqlx::query!(
        "UPDATE bookings SET amount = ?, date = ?, note = ?, club_id = ?, ignored = ? WHERE id = ?",
        input.amount,
        input.date,
        input.note,
        input.club_id,
        input.ignored,
        id,
    )
    .execute(&mut *tx)
    .await?;
    let after = Booking::load(&mut tx, id)
        .await?
        .ok_or(HttpError::InternalServerError)?;
    // Saving the form unchanged changes nothing to log.
    if after != before {
        bookings::log_change(
            &mut tx,
            id,
            &Actor::of(&granted),
            BookingAction::of_edit(&before, &after),
            Some(&before),
            Some(&after),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(EditBookingResponse {}))
}
