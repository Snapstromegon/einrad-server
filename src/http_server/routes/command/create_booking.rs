use axum::{Extension, Json, http::StatusCode};
use serde::Serialize;
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
pub struct CreateBookingResponse {
    id: Uuid,
}

/// Record a booking: a payment of a club (a refund when negative), an
/// unassigned or an ignored booking. Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/create_booking",
    request_body=BookingInput,
    responses(
        (status=200, content_type="application/json", body=CreateBookingResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn create_booking(
    Extension(db): Extension<ReloadableSqlite>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<BookingInput>,
) -> Result<Json<CreateBookingResponse>, HttpError> {
    let input = body
        .checked()
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    if let Some(club_id) = input.club_id
        && sqlx::query_scalar!("SELECT 1 as one FROM clubs WHERE id = ?", club_id)
            .fetch_optional(&mut *tx)
            .await?
            .is_none()
    {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    let id = Uuid::now_v7();
    info!(
        "Booking {id}: {} € for club {:?}",
        input.amount, input.club_id
    );
    sqlx::query!(
        "INSERT INTO bookings (id, amount, date, note, club_id, ignored) VALUES (?, ?, ?, ?, ?, ?)",
        id,
        input.amount,
        input.date,
        input.note,
        input.club_id,
        input.ignored,
    )
    .execute(&mut *tx)
    .await?;
    let after = Booking::load(&mut tx, id).await?;
    bookings::log_change(
        &mut tx,
        id,
        &Actor::of(&granted),
        BookingAction::Created,
        None,
        after.as_ref(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(CreateBookingResponse { id }))
}
