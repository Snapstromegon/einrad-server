use crate::ids::{BookingId, ClubId, UserId};
use axum::{Extension, Json, extract::Query};
use db_core::{prelude::*, queries::club::GetClub};
use tracing::instrument;

use crate::{
    bookings::round_cents,
    http_server::{
        ClientError, HttpError,
        extractor::auth::Auth,
        routes::http_types::{AmountDue, JuryObligation},
    },
    utils::club_code,
};

/// One payment of a club, as the club sees it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Payment {
    id: BookingId,
    /// Negative for a refund.
    amount: f64,
    #[serde(with = "crate::bookings::iso_date")]
    date: time::Date,
    note: String,
}

impl From<db_core::bookings::Payment> for Payment {
    fn from(payment: db_core::bookings::Payment) -> Self {
        Self {
            id: payment.id,
            amount: payment.amount,
            date: payment.date,
            note: payment.note,
        }
    }
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Club {
    id: ClubId,
    name: String,
    owner_id: UserId,
    /// The sum of the club's payments. Only for the club itself and the
    /// organizer.
    paid: Option<f64>,
    /// The club's payments, oldest first; refunds are negative. Only for the
    /// club itself and the organizer.
    payments: Option<Vec<Payment>>,
    /// Short code for the transfer reference, e.g. "EV-K7Q3"; stays the
    /// same when the club is renamed.
    code: String,
    /// The user who registered the club. Only for the club itself and
    /// the organizer.
    owner_name: Option<String>,
    owner_email: Option<String>,
    /// The Vereinsverantwortliche/r, who files objections; the owner unless
    /// the club named someone else. Only for the club itself and the organizer.
    responsible_name: Option<String>,
    responsible_email: Option<String>,
    /// What the club owes. Only for the club itself and the organizer.
    amount_due: Option<AmountDue>,
    /// All the club's fees are waived. Only for the club itself and the
    /// organizer; the reason isn't shown here.
    fee_waived: Option<bool>,
    /// The club's jury obligation, while the jury registration is on. Only
    /// for the club itself and the organizer.
    jury: Option<JuryObligation>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetClubQuery {
    club_id: Option<ClubId>,
}

/// Get information about a club. The owner's and the Vereinsverantwortliche's
/// name and email address are only included for the club itself and the organizer.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/get_club",
    params(GetClubQuery),
    responses(
        (status=200, content_type="application/json", body=Club),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn get_club(
    Extension(repo): Extension<DbHandle>,
    Query(query): Query<GetClubQuery>,
    auth: Option<Auth>,
) -> Result<Json<Club>, HttpError> {
    let actor = auth.as_ref().map(Auth::actor);
    let club_id = query
        .club_id
        .or_else(|| auth.as_ref().and_then(|auth| auth.club_id))
        .ok_or(HttpError::NotFound)?;
    let view = repo.get_club(actor.as_ref(), GetClub { club_id }).await?;
    let row = view.profile;
    let mut club = Club {
        id: row.id,
        name: row.name,
        owner_id: row.owner_id,
        paid: None,
        payments: None,
        code: club_code(&row.id),
        owner_name: row.owner_name,
        owner_email: row.owner_email,
        responsible_name: row.responsible_name,
        responsible_email: row.responsible_email,
        amount_due: None,
        fee_waived: None,
        jury: None,
    };
    match view.details {
        Some(details) => {
            club.paid = Some(round_cents(details.payments.iter().map(|p| p.amount).sum()));
            club.payments = Some(details.payments.into_iter().map(Payment::from).collect());
            club.amount_due = Some(details.fees.amount_due.into());
            club.fee_waived = Some(row.fee_waived);
            club.jury = details.fees.jury.map(Into::into);
        }
        None => {
            club.owner_name = None;
            club.owner_email = None;
            club.responsible_name = None;
            club.responsible_email = None;
        }
    }
    Ok(Json(club))
}
