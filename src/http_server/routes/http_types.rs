use crate::ids::{ActId, CategoryId, ClubId, HeatId, JudgeId, JuryUnitId};
use crate::{act_type::ActType, roles::Permission};
use db_core::acts::ActRecord;
use db_core::jury::JudgeTask;
use time::OffsetDateTime;

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

/// A run of slots as the admin API takes it: the heat by id.
#[derive(Debug, Clone, Copy, serde::Deserialize, utoipa::ToSchema)]
pub struct SlotGroupInput {
    /// The heat whose best acts fill these slots.
    pub heat_id: HeatId,
    pub count: i64,
}

// Shared by `add_final` and `edit_final`.
/// What admins set for a final, apart from its name.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct FinalFields {
    pub description: Option<String>,
    /// The discipline: act type and, for singles, male or female.
    pub act_type: ActType,
    #[serde(default)]
    pub is_single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    /// The reserved slots in start order, e.g. 5 × "JE EM".
    pub slot_groups: Vec<SlotGroupInput>,
}

impl From<FinalFields> for db_core::finals::FinalFields {
    fn from(fields: FinalFields) -> Self {
        Self {
            description: fields.description,
            act_type: fields.act_type,
            is_single_male: fields.is_single_male,
            einfahrzeit_seconds: fields.einfahrzeit_seconds,
            act_duration_seconds: fields.act_duration_seconds,
            judge_duration_seconds: fields.judge_duration_seconds,
            slot_groups: fields
                .slot_groups
                .into_iter()
                .map(|group| db_core::finals::SlotGroupInput {
                    heat_id: group.heat_id,
                    count: group.count,
                })
                .collect(),
        }
    }
}

/// The kind of a timeplan entry, stored as text in `timeplan.kind`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A start block: acts and breaks.
    Acts,
    /// A break, an award ceremony, …: a label and a duration.
    Custom,
}

impl From<EntryKind> for db_core::timeplan::EntryKind {
    fn from(kind: EntryKind) -> Self {
        match kind {
            EntryKind::Acts => Self::Acts,
            EntryKind::Custom => Self::Custom,
        }
    }
}

impl From<db_core::timeplan::EntryKind> for EntryKind {
    fn from(kind: db_core::timeplan::EntryKind) -> Self {
        use db_core::timeplan::EntryKind as Db;
        match kind {
            Db::Acts => Self::Acts,
            Db::Custom => Self::Custom,
        }
    }
}

// ── jury ──

/// Which kind of jury unit: a heat's Vorlauf or a final.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum JuryUnitKind {
    /// The Vorlauf of a heat.
    Heat,
    /// A final.
    Final,
}

impl From<JuryUnitKind> for db_core::jury::JuryUnitKind {
    fn from(kind: JuryUnitKind) -> Self {
        match kind {
            JuryUnitKind::Heat => Self::Heat,
            JuryUnitKind::Final => Self::Final,
        }
    }
}

impl From<db_core::jury::JuryUnitKind> for JuryUnitKind {
    fn from(kind: db_core::jury::JuryUnitKind) -> Self {
        use db_core::jury::JuryUnitKind as Db;
        match kind {
            Db::Heat => Self::Heat,
            Db::Final => Self::Final,
        }
    }
}

/// A judge's task in a jury unit, as a club registers it.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
pub struct JudgeRegistration {
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

impl From<JudgeRegistration> for db_core::jury::JudgeRegistration {
    fn from(registration: JudgeRegistration) -> Self {
        Self {
            unit_kind: registration.unit_kind.into(),
            unit_id: registration.unit_id,
            task: registration.task,
        }
    }
}

// Shared by `add_club_judge` and `edit_club_judge`.
/// What clubs set for a judge.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct JudgeFields {
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: OffsetDateTime,
    pub is_trained: bool,
    #[serde(default)]
    pub max_deployments: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub registrations: Vec<JudgeRegistration>,
}

impl From<JudgeFields> for db_core::jury::JudgeFields {
    fn from(fields: JudgeFields) -> Self {
        Self {
            firstname: fields.firstname,
            lastname: fields.lastname,
            birthdate: fields.birthdate,
            is_trained: fields.is_trained,
            max_deployments: fields.max_deployments,
            note: fields.note,
            registrations: fields.registrations.into_iter().map(Into::into).collect(),
        }
    }
}

// Shared by `assign_judge` (the body) and `list_jury_plan`.
/// A judge's task in a jury unit of the Juryplan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JuryAssignment {
    pub judge_id: JudgeId,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

impl From<db_core::queries::jury::JuryAssignment> for JuryAssignment {
    fn from(assignment: db_core::queries::jury::JuryAssignment) -> Self {
        Self {
            judge_id: assignment.judge_id,
            unit_kind: assignment.unit_kind.into(),
            unit_id: assignment.unit_id,
            task: assignment.task,
        }
    }
}

// Shared by `list_club_judges` and `list_judges`.
/// A registration as listed: with whether the jury lead has to move it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ListedRegistration {
    pub unit_kind: JuryUnitKind,
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
    /// Its unit takes no registrations (a skipped Vorlauf, or switched off):
    /// it still counts, but the jury lead should move it (D7).
    pub flagged: bool,
}

/// A registration of a migrated jury category that matched no heat or final
/// (JURY-5), for the jury lead to move to one. It counts for nothing meanwhile.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct UnmappedRegistration {
    pub id: i64,
    /// The name of the old jury category.
    pub old_unit: String,
    pub task: JudgeTask,
}

/// A club's judge with what they registered for.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ClubJudge {
    pub id: JudgeId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: OffsetDateTime,
    pub is_trained: bool,
    pub max_deployments: Option<i64>,
    pub note: Option<String>,
    pub registrations: Vec<ListedRegistration>,
    pub unmapped_registrations: Vec<UnmappedRegistration>,
    /// What the jury units they registered for are worth (each once).
    pub registered_deployments: i64,
    /// What of it counts for the club: see [`counted_deployments`].
    pub counted_deployments: i64,
}

impl From<db_core::queries::jury::ClubJudge> for ClubJudge {
    fn from(judge: db_core::queries::jury::ClubJudge) -> Self {
        Self {
            id: judge.id,
            club_id: judge.club_id,
            club_name: judge.club_name,
            firstname: judge.firstname,
            lastname: judge.lastname,
            birthdate: judge.birthdate,
            is_trained: judge.is_trained,
            max_deployments: judge.max_deployments,
            note: judge.note,
            registrations: judge
                .registrations
                .into_iter()
                .map(|r| ListedRegistration {
                    unit_kind: r.unit_kind.into(),
                    unit_id: r.unit_id,
                    task: r.task,
                    flagged: r.flagged,
                })
                .collect(),
            unmapped_registrations: judge
                .unmapped_registrations
                .into_iter()
                .map(|r| UnmappedRegistration {
                    id: r.id,
                    old_unit: r.old_unit,
                    task: r.task,
                })
                .collect(),
            registered_deployments: judge.registered_deployments,
            counted_deployments: judge.counted_deployments,
        }
    }
}
