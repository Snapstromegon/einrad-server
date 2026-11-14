use crate::ids::{ActId, CategoryId, ClubId, RiderId, UserId};
use crate::{
    act_type::ActType,
    normalize::NormalizationStatus,
    roles::{Permission, Role},
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub email: String,
    pub email_verified: bool,
    /// Sorted.
    pub roles: Vec<Role>,
    pub club_id: Option<ClubId>,
}

/// The logged-in user with what their roles allow.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CurrentUser {
    #[serde(flatten)]
    pub user: User,
    /// Sorted.
    pub permissions: Vec<Permission>,
}

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

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Act {
    pub id: ActId,
    pub name: String,
    /// Group acts only.
    pub group_name: Option<String>,
    /// Group acts only: the club that created the group.
    pub owner_club_id: Option<ClubId>,
    pub song_file: Option<String>,
    pub description: Option<String>,
    pub song_file_name: Option<String>,
    /// The loudness-normalized copy of the song, once it is ready.
    pub song_file_normalized: Option<String>,
    /// `null` without a song.
    pub normalization_status: Option<NormalizationStatus>,
    pub song_checked: bool,
    pub act_type: ActType,
    /// Age of the oldest rider (replacements included) on the determining
    /// day. Only for who may see birthdates, `null` for everyone else: it
    /// would give away the rider's birthdate.
    pub max_age: Option<f64>,
    pub participants: Vec<ActParticipant>,
    /// The category's ID and name; `null` for acts no category matches,
    /// incomplete groups (fewer than 3 regular riders) and groups with a
    /// `type_conflict`.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Groups only: a rider is also in another group of the same type (a rider
    /// may start in one small and one large group). Such groups get no
    /// category until that's sorted out.
    pub type_conflict: bool,
    /// Withdrawn after the registration deadline: the act doesn't start and
    /// has no category, but its riders still pay for it unless `fee_waived`.
    pub withdrawn: bool,
    /// The act's fee is waived: its start is free for every rider.
    pub fee_waived: bool,
    /// Why the fee is waived; only for who manages payments.
    pub fee_waiver: Option<String>,
    /// Start order within the category; hidden until the start list is
    /// published.
    pub act_order: Option<i64>,
    pub category_order: Option<i64>,
}

impl Act {
    /// Hides data only who may see birthdates may see, see [`Act::max_age`],
    /// the waiver reason from who doesn't manage payments, and the start order
    /// from who may not see the start list yet.
    pub fn for_viewer(
        mut self,
        sees_birthdates: bool,
        manages_payments: bool,
        sees_startlist: bool,
    ) -> Self {
        if !sees_startlist {
            self.act_order = None;
        }
        if !sees_birthdates {
            self.max_age = None;
        }
        if !manages_payments {
            self.fee_waiver = None;
        }
        self
    }
}

/// A row of `view_act` (left-joined with its category) as read by `list_acts`
/// and `get_act`.
pub struct DBAct {
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
    pub participants: sqlx::types::Json<Vec<ActParticipant>>,
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub type_conflict: bool,
    pub song_checked: bool,
    pub withdrawn: bool,
    pub fee_waiver: Option<String>,
    pub act_order: Option<i64>,
    pub category_order: Option<i64>,
}

impl From<DBAct> for Act {
    fn from(db_act: DBAct) -> Self {
        Act {
            id: db_act.id,
            name: db_act.name,
            group_name: db_act.group_name,
            owner_club_id: db_act.owner_club_id,
            song_file: db_act.song_file,
            description: db_act.description,
            song_file_name: db_act.song_file_name,
            // Only a finished copy is worth playing.
            song_file_normalized: db_act
                .song_file_normalized
                .filter(|_| db_act.normalization_status == Some(NormalizationStatus::Done)),
            normalization_status: db_act.normalization_status,
            act_type: db_act.act_type,
            max_age: db_act.max_age,
            participants: db_act.participants.0,
            category_id: db_act.category_id,
            category: db_act.category,
            type_conflict: db_act.type_conflict,
            song_checked: db_act.song_checked,
            withdrawn: db_act.withdrawn,
            fee_waived: db_act.fee_waiver.is_some(),
            fee_waiver: db_act.fee_waiver,
            act_order: db_act.act_order,
            category_order: db_act.category_order,
        }
    }
}
