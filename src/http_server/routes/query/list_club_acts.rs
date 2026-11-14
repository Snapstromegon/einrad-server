use crate::ids::{ActId, ClubId, RiderId};
use std::sync::Arc;

use axum::{Extension, Json, extract::Query};
use db_core::{DbHandle, acts::NormalizationStatus};
use tracing::instrument;

use crate::{
    act_type::ActType,
    competition::Features,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubAct {
    id: ActId,
    name: String,
    /// Group acts only.
    group_name: Option<String>,
    /// Group acts only: the club that created the group.
    owner_club_id: Option<ClubId>,
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
    id: RiderId,
    rider_firstname: String,
    rider_lastname: String,
    club_id: ClubId,
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
    club_id: ClubId,
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
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn list_club_acts(
    Extension(repo): Extension<DbHandle>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    Query(query): Query<ListClubActsQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubAct>>, HttpError> {
    repo.ensure_club_access(&auth.actor(), query.club_id)
        .await?;
    let moderation_input = Features::from_repo(repo.as_ref()).await?.moderation_input;
    let acts = repo.list_club_acts(query.club_id).await?;

    let club_acts: Vec<ClubAct> = acts
        .into_iter()
        .map(|act| ClubAct {
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
            participants: act
                .participants
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
        })
        .collect();

    Ok(Json(club_acts))
}
