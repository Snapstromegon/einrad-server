//! Acts: the [`ActsRepo`] trait, the rows it returns and the song metadata.

use std::path::Path;

use crate::{
    DbError,
    act_type::ActType,
    ids::{ActId, CategoryId, ClubId, RiderId},
};

/// A participant of an act, as stored in the `participants` JSON of `view_act`.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema, Clone)]
pub struct ActParticipant {
    pub firstname: String,
    pub lastname: String,
    pub id: RiderId,
    pub club_id: ClubId,
    pub club_name: String,
    /// Replacement riders (Ersatzfahrer) of a group. Always false for singles
    /// and pairs.
    pub is_replacement: bool,
}

/// Where the loudness normalization of a song stands.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    utoipa::ToSchema,
    sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum NormalizationStatus {
    Pending,
    Done,
    Failed,
}

/// An act with its category, as `list_acts` and `get_act` show it.
#[derive(Debug, Clone)]
pub struct ActRecord {
    pub id: ActId,
    pub name: String,
    pub group_name: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub song_file: Option<String>,
    pub description: Option<String>,
    pub song_file_name: Option<String>,
    pub song_file_normalized: Option<String>,
    pub normalization_status: Option<NormalizationStatus>,
    pub act_type: ActType,
    pub max_age: Option<f64>,
    pub participants: Vec<ActParticipant>,
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub type_conflict: bool,
    pub song_checked: bool,
    pub withdrawn: bool,
    pub fee_waiver: Option<String>,
    pub act_order: Option<i64>,
    pub category_order: Option<i64>,
}

/// A participant of a startlist act: no ages, no club ids.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema, Clone)]
pub struct StartlistActParticipant {
    pub firstname: String,
    pub lastname: String,
    pub id: RiderId,
    pub club_name: String,
    /// Replacement riders (Ersatzfahrer) of a group.
    pub is_replacement: bool,
}

/// An act on the public startlist.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct StartlistAct {
    pub id: ActId,
    pub name: String,
    /// Group acts only.
    pub group_name: Option<String>,
    pub act_type: ActType,
    pub participants: Vec<StartlistActParticipant>,
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub act_order: Option<i64>,
    pub category_order: Option<i64>,
}

/// An act of a club with the data the club's own page shows.
#[derive(Debug, Clone)]
pub struct ClubActRecord {
    pub id: ActId,
    pub name: String,
    pub group_name: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub description: Option<String>,
    pub song_file_name: Option<String>,
    pub song_file: Option<String>,
    pub song_file_normalized: Option<String>,
    pub normalization_status: Option<NormalizationStatus>,
    pub act_type: ActType,
    pub created_at: time::OffsetDateTime,
    pub withdrawn: bool,
    pub fee_waived: bool,
    pub participants: Vec<ClubActParticipantRecord>,
}

#[derive(Debug, Clone)]
pub struct ClubActParticipantRecord {
    pub id: RiderId,
    pub firstname: String,
    pub lastname: String,
    pub club_id: ClubId,
    pub club_name: String,
    pub is_replacement: bool,
    pub birthdate: time::OffsetDateTime,
}

/// An act with an uploaded song, and where its file should be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActSong {
    pub act_id: ActId,
    /// The act's name, or the group's.
    pub name: String,
    /// The act's club(s), if known.
    pub club: Option<String>,
    /// The file name of the original upload.
    pub song_file_name: Option<String>,
    /// The file, relative to the data directory.
    pub song_file: String,
}

/// An act whose song file isn't in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct MissingSong {
    pub act_id: ActId,
    /// The act's name, or the group's.
    pub name: String,
    /// The act's club(s), if known.
    pub club: Option<String>,
    /// The file name of the original upload.
    pub song_file_name: Option<String>,
    /// The expected file, relative to the data directory.
    pub song_file: String,
}

/// The songs among `songs` whose file isn't in `data_path`.
#[must_use]
pub fn missing_songs(songs: Vec<ActSong>, data_path: &Path) -> Vec<MissingSong> {
    songs
        .into_iter()
        .filter(|song| !data_path.join(&song.song_file).is_file())
        .map(|song| MissingSong {
            act_id: song.act_id,
            name: song.name,
            club: song.club,
            song_file_name: song.song_file_name,
            song_file: song.song_file,
        })
        .collect()
}

/// A song that still needs a normalized copy.
#[derive(Debug, Clone)]
pub struct SongFiles {
    pub act_id: ActId,
    pub song_file: String,
    pub song_file_normalized: Option<String>,
}

#[async_trait::async_trait]
pub trait ActsRepo {
    /// All acts, by category and act order; acts without a category last.
    async fn list_acts(&self) -> Result<Vec<ActRecord>, DbError>;

    async fn find_act(&self, id: ActId) -> Result<Option<ActRecord>, DbError>;

    /// The acts with a category that start, in start order.
    async fn list_startlist(&self) -> Result<Vec<StartlistAct>, DbError>;

    /// Every act with a rider of the club or created by it, with
    /// participants. Unfiltered: what the club may see is up to the caller.
    async fn list_club_acts(&self, club_id: ClubId) -> Result<Vec<ClubActRecord>, DbError>;

    /// Renames an act. `description` is only stored when `set_description`.
    async fn edit_act(
        &self,
        id: ActId,
        name: &str,
        set_description: bool,
        description: Option<&str>,
    ) -> Result<(), DbError>;

    /// Withdraws an act (or takes it back). `false` if the act doesn't exist.
    async fn set_act_withdrawn(&self, id: ActId, withdrawn: bool) -> Result<bool, DbError>;

    async fn set_act_order(&self, id: ActId, order: Option<i64>) -> Result<(), DbError>;

    async fn set_song_checked(&self, id: ActId, checked: bool) -> Result<(), DbError>;

    /// Forgets the act's song. `false` if the act doesn't exist.
    async fn clear_act_song(&self, id: ActId) -> Result<bool, DbError>;

    /// Stores a new upload: the song is unchecked and its normalization
    /// pending as `normalized`. Returns the normalized copy it replaced.
    async fn save_act_song(
        &self,
        id: ActId,
        file_name: &str,
        song_file: &str,
        normalized: &str,
    ) -> Result<Option<String>, DbError>;

    /// Records the normalization result, unless the act got another upload
    /// (a different `normalized` name) meanwhile. `true` if recorded.
    async fn finish_normalization(
        &self,
        id: ActId,
        normalized: &str,
        status: NormalizationStatus,
    ) -> Result<bool, DbError>;

    /// Songs without a finished or running normalization.
    async fn list_songs_to_normalize(&self) -> Result<Vec<SongFiles>, DbError>;

    /// Every act with a song and its normalized copy.
    async fn list_song_files(&self) -> Result<Vec<SongFiles>, DbError>;

    /// Points the act at a new normalized copy that is pending.
    async fn queue_normalization(&self, id: ActId, normalized: &str) -> Result<(), DbError>;

    /// Every act with a song, by name, with its club(s).
    async fn list_act_songs(&self) -> Result<Vec<ActSong>, DbError>;
}
