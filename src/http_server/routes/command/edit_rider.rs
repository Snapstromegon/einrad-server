use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use super::add_rider::{partner_clubs, single_from_flags};
use crate::{
    gender::Gender,
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    people::{self, Person},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration::{self, Pair},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditRiderBody {
    rider_id: Uuid,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Missing keeps the stored gender (or takes the single's).
    #[serde(default)]
    gender: Option<Gender>,
    single_male: bool,
    single_female: bool,
    pair: bool,
    partner_name: Option<String>,
    partner_id: Option<Uuid>,
}

/// Edit a rider.
///
/// Only the rider's club (or an admin) can edit them. A newly chosen
/// `partner_id` must be a rider of the same club; admins can also link
/// partners of other clubs. A linked partner is kept while the body names
/// them (by `partner_id` or `partner_name`); another `partner_name` ends the
/// pair and asks for that partner instead.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_rider",
    request_body=EditRiderBody,
    responses(
        (status=200, content_type="application/json", body=EditRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_rider(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<EditRiderBody>,
) -> Result<Json<EditRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let (single, gender) = single_from_flags(body.single_male, body.single_female, body.gender)?;
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let club_id = ensure_rider_access(&mut transaction, &auth, body.rider_id).await?;
    people::ensure_name_is_free(
        &mut transaction,
        Person::Rider,
        club_id,
        &body.firstname,
        &body.lastname,
        Some(body.rider_id),
    )
    .await?;

    // Clients without the field (the admin's rider table) keep the gender;
    // a single of the other gender then changes it.
    let stored_gender = sqlx::query_scalar!(
        r#"SELECT gender as "gender: Gender" FROM rider WHERE id = ?"#,
        body.rider_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    let gender = gender.or(stored_gender);
    sqlx::query!(
        r#"
        UPDATE rider SET firstname = ?, lastname = ?, birthdate = ?, gender = ?
        WHERE id = ?
        "#,
        body.firstname,
        body.lastname,
        body.birthdate,
        gender,
        body.rider_id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| people::map_duplicate(e, &body.firstname, &body.lastname))?;

    rider_registration::set_single(&mut transaction, body.rider_id, single).await?;

    let partner_name = body
        .partner_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let starts = rider_registration::starts(&mut transaction, body.rider_id).await?;
    let linked = match starts.pair {
        Pair::Linked { partner, .. } => partner,
        Pair::None | Pair::Requested { .. } => None,
    };
    let linked_name = match linked {
        Some(partner) => Some(
            sqlx::query_scalar!(
                r#"SELECT concat_ws(' ', firstname, lastname) as "name!: String" FROM rider WHERE id = ?"#,
                partner,
            )
            .fetch_one(&mut *transaction)
            .await?,
        ),
        None => None,
    };
    let keeps_partner = linked.is_some()
        && match body.partner_id {
            Some(partner_id) if Some(partner_id) == linked => {
                partner_name.is_none() || partner_name == linked_name.as_deref()
            }
            Some(_) => false,
            None => partner_name.is_some() && partner_name == linked_name.as_deref(),
        };
    if !body.pair {
        rider_registration::leave_pair(&mut transaction, body.rider_id).await?;
    } else if !keeps_partner {
        match body.partner_id.filter(|id| Some(*id) != linked) {
            Some(partner_id) => {
                rider_registration::leave_pair(&mut transaction, body.rider_id).await?;
                rider_registration::link_pair(
                    &mut transaction,
                    body.rider_id,
                    partner_id,
                    partner_clubs(&auth),
                )
                .await?;
            }
            None => {
                if linked.is_some() {
                    rider_registration::leave_pair(&mut transaction, body.rider_id).await?;
                }
                rider_registration::request_pair(&mut transaction, body.rider_id, partner_name)
                    .await?;
            }
        }
    }

    transaction.commit().await?;
    Ok(Json(EditRiderResponse {}))
}
