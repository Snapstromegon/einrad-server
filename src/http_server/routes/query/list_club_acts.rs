use std::sync::Arc;

use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    competition::{Competition, Features},
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    normalize::NormalizationStatus,
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubAct {
    id: Uuid,
    name: String,
    /// Group acts only.
    group_name: Option<String>,
    /// Group acts only: the club that created the group.
    owner_club_id: Option<Uuid>,
    description: Option<String>,
    song_file_name: Option<String>,
    song_file: Option<String>,
    /// The loudness-normalized copy of the song, once it is ready. Clubs can
    /// listen to both versions.
    song_file_normalized: Option<String>,
    /// `null` without a song.
    normalization_status: Option<NormalizationStatus>,
    act_type: ActType,
    #[serde(with = "time::serde::iso8601")]
    created_at: time::OffsetDateTime,
    participants: Vec<ClubActParticipant>,
    /// Withdrawn after the registration deadline: the act doesn't start and
    /// has no category, but its riders still pay for it unless `fee_waived`.
    withdrawn: bool,
    /// The act's fee is waived: its start is free for every rider.
    fee_waived: bool,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubActParticipant {
    id: Uuid,
    rider_firstname: String,
    rider_lastname: String,
    club_id: Uuid,
    club_name: String,
    /// Replacement riders (Ersatzfahrer) of a group.
    is_replacement: bool,
    /// Only for the club's own riders. Groups can contain riders of other
    /// clubs, whose birthdates aren't shared.
    #[serde(with = "time::serde::iso8601::option")]
    birthdate: Option<time::OffsetDateTime>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubActsQuery {
    club_id: Uuid,
}

/// List the acts of a club.
///
/// All acts with at least one of the club's riders, plus the groups the club
/// created (even without its own riders). Only for the club and admins.
#[utoipa::path(
    get,
    tags=["query", "club"],
    params(ListClubActsQuery),
    path="/list_club_acts",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubAct>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn list_club_acts(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Query(query): Query<ListClubActsQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubAct>>, HttpError> {
    let db = db.get().await.clone();
    let mut connection = db.acquire().await?;
    ensure_club_access(&mut connection, &auth, query.club_id).await?;
    let moderation_input = Features::of(Competition::current(&db).await?.as_ref()).moderation_input;
    let acts_with_club_participation = sqlx::query!(
        r#"
        SELECT
            acts.id as "id!: Uuid",
            acts.name,
            acts.group_name,
            acts.owner_club_id as "owner_club_id: Uuid",
            acts.description,
            acts.song_file_name,
            acts.song_file,
            acts.song_file_normalized,
            acts.normalization_status as "normalization_status: NormalizationStatus",
            acts.act_type as "act_type: ActType",
            acts.created_at as "created_at!: time::OffsetDateTime",
            acts.withdrawn_at IS NOT NULL as "withdrawn!: bool",
            acts.fee_waiver IS NOT NULL as "fee_waived!: bool"
        FROM acts
        WHERE acts.owner_club_id = ?1
            OR EXISTS (
                SELECT 1 FROM act_participants
                JOIN rider ON act_participants.rider_id = rider.id
                WHERE act_participants.act_id = acts.id AND rider.club_id = ?1
            )
        ORDER BY acts.id
        "#,
        query.club_id
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut club_acts: Vec<ClubAct> = Vec::with_capacity(acts_with_club_participation.len());
    for act in acts_with_club_participation {
        let participants = sqlx::query!(
            r#"
            SELECT
                rider.id as "id!: Uuid",
                rider.firstname,
                rider.lastname,
                rider.club_id as "club_id!: Uuid",
                clubs.name as club_name,
                act_participants.is_replacement,
                rider.birthdate
            FROM act_participants
            JOIN rider ON act_participants.rider_id = rider.id
            JOIN clubs ON clubs.id = rider.club_id
            WHERE act_participants.act_id = ?
            ORDER BY act_participants.is_replacement, rider.lastname, rider.firstname
            "#,
            act.id
        )
        .fetch_all(&mut *connection)
        .await?;

        club_acts.push(ClubAct {
            id: act.id,
            name: act.name,
            group_name: act.group_name,
            owner_club_id: act.owner_club_id,
            description: moderation_input.then_some(act.description).flatten(),
            song_file_name: act.song_file_name,
            song_file: act.song_file,
            song_file_normalized: act
                .song_file_normalized
                .filter(|_| act.normalization_status == Some(NormalizationStatus::Done)),
            normalization_status: act.normalization_status,
            act_type: act.act_type,
            created_at: act.created_at,
            withdrawn: act.withdrawn,
            fee_waived: act.fee_waived,
            participants: participants
                .into_iter()
                .map(|participant| ClubActParticipant {
                    id: participant.id,
                    rider_firstname: participant.firstname,
                    rider_lastname: participant.lastname,
                    club_id: participant.club_id,
                    club_name: participant.club_name,
                    is_replacement: participant.is_replacement,
                    birthdate: (participant.club_id == query.club_id)
                        .then_some(participant.birthdate),
                })
                .collect(),
        });
    }

    Ok(Json(club_acts))
}
