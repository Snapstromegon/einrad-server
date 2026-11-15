use crate::ids::{ActId, CategoryId, ClubId};
use crate::{act_type::ActType, roles::Permission};
use db_core::acts::ActRecord;

pub use db_core::users::User;

// API types shared by several handlers. A schema name must exist exactly once
// (utoipa silently keeps one of two same-named structs), so a type that more
// than one handler returns is defined here, not in each handler file.

/// Where the loudness normalization of a song stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationStatus {
    Pending,
    Done,
    Failed,
}

impl From<db_core::acts::NormalizationStatus> for NormalizationStatus {
    fn from(status: db_core::acts::NormalizationStatus) -> Self {
        use db_core::acts::NormalizationStatus as Db;
        match status {
            Db::Pending => Self::Pending,
            Db::Done => Self::Done,
            Db::Failed => Self::Failed,
        }
    }
}

/// What a club owes, split into its parts.
#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct AmountDue {
    /// `fee_per_start` per start, at most `max_fee_per_rider` per rider.
    pub start_fees: f64,
    /// For missing jury deployments; 0 without the jury registration.
    pub jury_fee: f64,
    pub total: f64,
    /// What fee waivers drop: the start fees of waived clubs, riders and
    /// acts, and the jury fee of a waived club.
    pub waived: f64,
}

impl From<db_core::fees::AmountDue> for AmountDue {
    fn from(due: db_core::fees::AmountDue) -> Self {
        Self {
            start_fees: due.start_fees,
            jury_fee: due.jury_fee,
            total: due.total,
            waived: due.waived,
        }
    }
}

/// A club's jury obligation.
#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct JuryObligation {
    /// Küren that count: those charged a start fee, for the owner club.
    pub acts: i64,
    pub required: i64,
    /// What the club's trained judges registered for as technical or
    /// performance judges.
    pub registered: i64,
    /// What an admin credits instead, e.g. after a late cancellation.
    pub credited_override: Option<i64>,
    /// `credited_override`, or `registered` without one.
    pub credited: i64,
    pub missing: i64,
    /// For the missing deployments, in €.
    pub fee: f64,
}

impl From<db_core::jury::JuryObligation> for JuryObligation {
    fn from(jury: db_core::jury::JuryObligation) -> Self {
        Self {
            acts: jury.acts,
            required: jury.required,
            registered: jury.registered,
            credited_override: jury.credited_override,
            credited: jury.credited,
            missing: jury.missing,
            fee: jury.fee,
        }
    }
}

/// The logged-in user with what their roles allow.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CurrentUser {
    #[serde(flatten)]
    pub user: User,
    /// Sorted.
    pub permissions: Vec<Permission>,
}

pub use db_core::acts::ActParticipant;

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

impl From<ActRecord> for Act {
    fn from(db_act: ActRecord) -> Self {
        Act {
            id: db_act.id,
            name: db_act.name,
            group_name: db_act.group_name,
            owner_club_id: db_act.owner_club_id,
            song_file: db_act.song_file,
            description: db_act.description,
            song_file_name: db_act.song_file_name,
            // Only a finished copy is worth playing.
            song_file_normalized: db_act.song_file_normalized.filter(|_| {
                db_act.normalization_status == Some(db_core::acts::NormalizationStatus::Done)
            }),
            normalization_status: db_act.normalization_status.map(Into::into),
            act_type: db_act.act_type,
            max_age: db_act.max_age,
            participants: db_act.participants,
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

/// A booking as the organizer enters it.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct BookingInput {
    /// Negative for money going out, e.g. a refund; not 0.
    pub amount: f64,
    #[serde(with = "crate::bookings::iso_date")]
    pub date: time::Date,
    /// E.g. the transfer's reference.
    #[serde(default)]
    pub note: String,
    /// The club this booking is a payment of; `null` if unassigned or ignored.
    pub club_id: Option<ClubId>,
    /// Has nothing to do with the event; then without a club.
    #[serde(default)]
    pub ignored: bool,
}

impl BookingInput {
    /// The input checked and normalized (amount in whole cents, note
    /// trimmed), or what is wrong with it.
    pub fn checked(self) -> Result<db_core::bookings::BookingInput, &'static str> {
        db_core::bookings::BookingInput {
            amount: self.amount,
            date: self.date,
            note: self.note,
            club_id: self.club_id,
            ignored: self.ignored,
        }
        .checked()
    }
}
