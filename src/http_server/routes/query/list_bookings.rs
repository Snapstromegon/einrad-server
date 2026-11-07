use axum::{Extension, Json};
use time::Date;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ListedBooking {
    id: Uuid,
    /// Negative for money going out, e.g. a refund.
    amount: f64,
    #[serde(with = "iso_date")]
    date: Date,
    note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    club_id: Option<Uuid>,
    club_name: Option<String>,
    ignored: bool,
}

/// List all bookings, newest first: payments, unassigned and ignored ones.
/// Treasurer.
#[utoipa::path(
    get,
    tags=["query", "payment"],
    path="/list_bookings",
    responses(
        (status=200, content_type="application/json", body=Vec<ListedBooking>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_bookings(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManagePayments>,
) -> Result<Json<Vec<ListedBooking>>, HttpError> {
    let db = db.get().await.clone();
    let bookings = sqlx::query_as!(
        ListedBooking,
        r#"
        SELECT
            bookings.id as "id!: Uuid",
            bookings.amount,
            bookings.date as "date: Date",
            bookings.note,
            bookings.club_id as "club_id: Uuid",
            clubs.name as "club_name?: String",
            bookings.ignored
        FROM bookings LEFT JOIN clubs ON clubs.id = bookings.club_id
        ORDER BY bookings.date DESC, bookings.rowid DESC
        "#
    )
    .fetch_all(&db)
    .await?;
    Ok(Json(bookings))
}
