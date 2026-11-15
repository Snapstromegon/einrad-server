//! Jury queries: jury units, judges and the jury plan.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::{Date, OffsetDateTime};

use crate::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{ClubId, FinalId, JudgeId, JuryUnitId},
    jury::{JudgeTask, JuryUnitKind},
    roles::Permission,
};

/// A club's judges with their registrations.
#[derive(Debug, Clone)]
pub struct ListClubJudges {
    pub club_id: ClubId,
}

/// The Juryplan: all of it, or one club's judges' assignments.
#[derive(Debug, Clone)]
pub struct ListJuryPlan {
    /// Only this club's judges. Required for everyone but the jury lead.
    pub club_id: Option<ClubId>,
}

/// Whether `actor` sees the whole Juryplan (all clubs, also while it is
/// unpublished): who manages the jury.
#[must_use]
pub fn sees_whole_jury_plan(actor: &Actor) -> bool {
    actor.can(Permission::ManageJury)
}

/// What judges register for and get assigned to: the Vorlauf of a heat or a
/// final (ADR 0016). Its settings live on the heat or the final.
#[derive(Debug, Clone, PartialEq)]
pub struct JuryUnit {
    /// The heat's or the final's id.
    pub id: JuryUnitId,
    pub kind: JuryUnitKind,
    /// The heat's or the final's readable name; renaming it keeps the
    /// registrations. Judges and the jury lead read this one.
    pub name: String,
    /// The heat's or the final's short code.
    pub code: String,
    pub description: Option<String>,
    /// Judges can register for it: its setting, and never for a skipped
    /// Vorlauf.
    pub registrable: bool,
    /// The jury lead's setting alone, which a skipped Vorlauf overrides.
    pub registrable_setting: bool,
    /// The heat's Vorlauf is skipped ("Vorlauf entfällt"). Registrations it
    /// already has are flagged for the jury lead to move.
    pub skipped: bool,
    /// Jury deployments (Einsätze) it counts for.
    pub deployments: i64,
    /// Judges it needs per task.
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
    /// The discipline: the act type of a heat's categories (the first one's
    /// if they differ, else the final it feeds) or of a final. `None` for a
    /// heat without categories that feeds no final.
    pub act_type: Option<ActType>,
    /// The final a heat feeds (one at most); `None` for a final and for a
    /// heat that feeds none.
    pub final_id: Option<FinalId>,
    /// The day it runs, derived from the timeplan; `None` while it isn't
    /// planned in it.
    pub day: Option<Date>,
}

/// A registration as listed: with whether the jury lead has to move it.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedRegistration {
    pub id: i64,
    /// The name of the old jury category.
    pub old_unit: String,
    pub task: JudgeTask,
}

/// A club's judge with what they registered for.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubJudge {
    pub id: JudgeId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    pub is_trained: bool,
    pub max_deployments: Option<i64>,
    pub note: Option<String>,
    pub registrations: Vec<ListedRegistration>,
    pub unmapped_registrations: Vec<UnmappedRegistration>,
    /// What the jury units they registered for are worth (each once).
    pub registered_deployments: i64,
    /// What of it counts for the club: see [`crate::jury::counted_deployments`].
    pub counted_deployments: i64,
}

/// A judge's task in a jury unit of the Juryplan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JuryAssignment {
    pub judge_id: JudgeId,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

/// The Juryplan as `list_jury_plan` returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JuryPlan {
    /// When the plan was published; `None` while only the jury lead sees it.
    pub published_at: Option<OffsetDateTime>,
    pub assignments: Vec<JuryAssignment>,
}

/// The read side of the jury area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait JuryQueries: Send + Sync {
    /// A club's judges with their registrations. `NotFound` for an unknown
    /// club, `Forbidden` unless the actor may manage it. One consistent read.
    async fn list_club_judges(
        &self,
        actor: &Actor,
        query: ListClubJudges,
    ) -> Result<Vec<ClubJudge>, QueryError>;

    /// The judges of all clubs, by club (the endpoint's role gate is
    /// the jury lead).
    async fn list_judges(&self, actor: &Actor) -> Result<Vec<ClubJudge>, QueryError>;

    /// The Juryplan, in one consistent read. With a club: that club's judges'
    /// assignments (`NotFound`/`Forbidden` like [`JuryQueries::list_club_judges`]);
    /// without one only for who [sees the whole plan](sees_whole_jury_plan)
    /// (`Forbidden` otherwise). Everyone else sees no assignments while the plan is
    /// unpublished.
    async fn list_jury_plan(
        &self,
        actor: &Actor,
        query: ListJuryPlan,
    ) -> Result<JuryPlan, QueryError>;

    /// The jury units (every heat's Vorlauf and every final) in the order of
    /// the timeplan, with the day they run on: by the first block of the
    /// heat's categories or of the final; units the timeplan doesn't run
    /// last, by name. Without a timeplan (or without a start time for its
    /// first block) there are no days.
    async fn list_jury_units(&self, actor: &Actor) -> Result<Vec<JuryUnit>, QueryError>;
}
