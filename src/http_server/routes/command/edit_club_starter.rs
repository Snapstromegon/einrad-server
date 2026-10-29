use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use super::add_club_starter::{partner_clubs, single_from_flags};
use crate::{
    gender::Gender,
    groups::ensure_starter_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    people::{self, Person},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration::{self, Pair},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditClubStarterResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditClubStarterBody {
    starter_id: Uuid,
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

/// Edit a starter.
///
/// Only the starter's club (or an admin) can edit them. A newly chosen
/// `partner_id` must be a starter of the same club; admins can also link
/// partners of other clubs. A linked partner is kept while the body names
/// them (by `partner_id` or `partner_name`); another `partner_name` ends the
/// pair and asks for that partner instead.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_club_starter",
    request_body=EditClubStarterBody,
    responses(
        (status=200, content_type="application/json", body=EditClubStarterResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_club_starter(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<EditClubStarterBody>,
) -> Result<Json<EditClubStarterResponse>, HttpError> {
    if !capabilities.can_register_starter {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let (single, gender) = single_from_flags(body.single_male, body.single_female, body.gender)?;
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let club_id = ensure_starter_access(&mut transaction, &auth, body.starter_id).await?;
    people::ensure_name_is_free(
        &mut transaction,
        Person::Rider,
        club_id,
        &body.firstname,
        &body.lastname,
        Some(body.starter_id),
    )
    .await?;

    // Clients without the field (the admin's starter table) keep the gender;
    // a single of the other gender then changes it.
    let stored_gender = sqlx::query_scalar!(
        r#"SELECT gender as "gender: Gender" FROM starter WHERE id = ?"#,
        body.starter_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    let gender = gender.or(stored_gender);
    sqlx::query!(
        r#"
        UPDATE starter SET firstname = ?, lastname = ?, birthdate = ?, gender = ?
        WHERE id = ?
        "#,
        body.firstname,
        body.lastname,
        body.birthdate,
        gender,
        body.starter_id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| people::map_duplicate(e, &body.firstname, &body.lastname))?;

    rider_registration::set_single(&mut transaction, body.starter_id, single).await?;

    let partner_name = body
        .partner_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let starts = rider_registration::starts(&mut transaction, body.starter_id).await?;
    let linked = match starts.pair {
        Pair::Linked { partner, .. } => partner,
        Pair::None | Pair::Requested { .. } => None,
    };
    let linked_name = match linked {
        Some(partner) => Some(
            sqlx::query_scalar!(
                r#"SELECT concat_ws(' ', firstname, lastname) as "name!: String" FROM starter WHERE id = ?"#,
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
        rider_registration::leave_pair(&mut transaction, body.starter_id).await?;
    } else if !keeps_partner {
        match body.partner_id.filter(|id| Some(*id) != linked) {
            Some(partner_id) => {
                rider_registration::leave_pair(&mut transaction, body.starter_id).await?;
                rider_registration::link_pair(
                    &mut transaction,
                    body.starter_id,
                    partner_id,
                    partner_clubs(&auth),
                )
                .await?;
            }
            None => {
                if linked.is_some() {
                    rider_registration::leave_pair(&mut transaction, body.starter_id).await?;
                }
                rider_registration::request_pair(&mut transaction, body.starter_id, partner_name)
                    .await?;
            }
        }
    }

    transaction.commit().await?;
    Ok(Json(EditClubStarterResponse {}))
}
