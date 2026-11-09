use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    http_server::{
        ClientError, HttpError,
        extractor::{auth::Granted, startlist::StartlistAccess},
        routes::http_types::{Act, ActParticipant, DBAct},
    },
    normalize::NormalizationStatus,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetActQuery {
    act_id: Uuid,
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
    Extension(db): Extension<ReloadableSqlite>,
    Query(query): Query<GetActQuery>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
    payments: Option<Granted<permit::ManagePayments>>,
    startlist: Option<StartlistAccess>,
) -> Result<Json<Act>, HttpError> {
    let db = db.get().await.clone();
    let act = sqlx::query_as!(
        DBAct,
        r#"
        SELECT
            view_act.id as "id!: Uuid",
            view_act."order" as "act_order",
            categories."order" as "category_order?",
            view_act.name,
            view_act.group_name,
            view_act.owner_club_id as "owner_club_id: Uuid",
            view_act.song_file,
            view_act.description,
            view_act.song_file_name,
            view_act.song_file_normalized,
            view_act.normalization_status as "normalization_status: NormalizationStatus",
            view_act.act_type as "act_type!: ActType",
            view_act.max_age as "max_age: f64",
            view_act.participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>",
            view_act.category,
            (
                view_act.act_type IN ('group_small', 'group_large')
                AND EXISTS (
                    SELECT 1 FROM act_participants own
                    JOIN act_participants theirs
                        ON theirs.rider_id = own.rider_id AND theirs.act_id != own.act_id
                    JOIN acts other ON other.id = theirs.act_id AND other.act_type = view_act.act_type
                    WHERE own.act_id = view_act.id
                )
            ) as "type_conflict!: bool",
            view_act.song_checked,
            view_act.withdrawn_at IS NOT NULL as "withdrawn!: bool",
            view_act.fee_waiver
        FROM view_act LEFT JOIN categories ON view_act.category = categories.name
        WHERE view_act.id = ?
        "#,
        query.act_id
    )
    .fetch_one(&db)
    .await?;
    Ok(Json(Act::from(act).for_viewer(
        birthdates.is_some(),
        payments.is_some(),
        startlist.is_some(),
    )))
}
