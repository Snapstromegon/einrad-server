//! Jury commands: jury units, judges and the jury plan.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them.

use time::OffsetDateTime;

use crate::{
    actor::Actor,
    error::CommandError,
    ids::{ClubId, JudgeId, JuryUnitId},
    jury::{JudgeFields, JudgeTask, JuryUnitFields, JuryUnitKind, RemapOutcome, RemapRegistration},
};

/// Registers a judge of a club.
#[derive(Debug, Clone)]
pub struct AddClubJudge {
    pub club_id: ClubId,
    pub fields: JudgeFields,
}

/// Changes a judge and replaces their registrations.
#[derive(Debug, Clone)]
pub struct EditClubJudge {
    pub judge_id: JudgeId,
    pub fields: JudgeFields,
}

/// Removes a judge with their registrations and assignments.
#[derive(Debug, Clone)]
pub struct DeleteClubJudge {
    pub judge_id: JudgeId,
}

/// Changes the jury settings of a unit (checked by [`JuryUnitFields::validate`]
/// inside the command).
#[derive(Debug, Clone)]
pub struct EditJuryUnit {
    pub fields: JuryUnitFields,
}

/// Assigns a judge a task in a unit of the Juryplan.
#[derive(Debug, Clone)]
pub struct AssignJudge {
    pub judge_id: JudgeId,
    pub unit_kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub unit_id: JuryUnitId,
    pub task: JudgeTask,
}

/// Removes the judge's assignments in a unit.
#[derive(Debug, Clone)]
pub struct UnassignJudge {
    pub judge_id: JudgeId,
    pub unit_id: JuryUnitId,
}

/// Moves all registrations of a judge from one unit to another.
#[derive(Debug, Clone)]
pub struct MoveJudgeRegistrations {
    pub judge_id: JudgeId,
    pub from_id: JuryUnitId,
    pub to_kind: JuryUnitKind,
    pub to_id: JuryUnitId,
}

/// Resolves a registration the migration to jury units could not map.
#[derive(Debug, Clone)]
pub struct ResolveUnmappedRegistration {
    /// The id of the unmapped registration.
    pub id: i64,
    /// The unit to register the judge for instead; `None` drops it.
    pub unit: Option<(JuryUnitKind, JuryUnitId)>,
}

/// Registers the judges of a remap file again.
#[derive(Debug, Clone)]
pub struct ApplyJuryRemap {
    pub registrations: Vec<RemapRegistration>,
}

/// Credits a club with jury deployments instead of the registered ones.
#[derive(Debug, Clone)]
pub struct SetJuryDeploymentsOverride {
    pub club_id: ClubId,
    /// `None` uses the registered ones again; never negative.
    pub deployments: Option<i64>,
}

/// Publishes the Juryplan (at `now`) or takes it back.
#[derive(Debug, Clone)]
pub struct SetJuryPlanPublished {
    pub published: bool,
    pub now: OffsetDateTime,
}

/// The write side of the jury area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait JuryCommands: Send + Sync {
    /// Registers a judge of the club. Normalizes the fields against the
    /// competition's jury rules and refuses (`Rejected`, German message)
    /// duplicate names, unknown jury units and units that take no
    /// registrations. `NotFound` for an unknown club, `Forbidden` unless the
    /// actor may manage it.
    async fn add_club_judge(
        &self,
        actor: &Actor,
        cmd: AddClubJudge,
    ) -> Result<JudgeId, CommandError>;

    /// Changes a judge and replaces their registrations; the club stays the
    /// same. An untrained judge also loses their Technik and Performance
    /// assignments. Same refusals as [`JuryCommands::add_club_judge`].
    async fn edit_club_judge(&self, actor: &Actor, cmd: EditClubJudge) -> Result<(), CommandError>;

    /// Removes a judge with their registrations and assignments.
    async fn delete_club_judge(
        &self,
        actor: &Actor,
        cmd: DeleteClubJudge,
    ) -> Result<(), CommandError>;

    /// Changes a unit's jury settings. `Rejected` for negative numbers,
    /// `NotFound` for an unknown unit.
    async fn edit_jury_unit(&self, actor: &Actor, cmd: EditJuryUnit) -> Result<(), CommandError>;

    /// Assigns a judge a task in a unit. `NotFound` for an unknown judge or
    /// unit, `Rejected` for an untrained judge with a task other than
    /// dismounts and for a duplicate.
    async fn assign_judge(&self, actor: &Actor, cmd: AssignJudge) -> Result<(), CommandError>;

    /// Removes the judge's assignments in a unit (none is fine).
    async fn unassign_judge(&self, actor: &Actor, cmd: UnassignJudge) -> Result<(), CommandError>;

    /// Moves all registrations of a judge from one unit to another; ones the
    /// judge already has there merge. `NotFound` for an unknown target or
    /// nothing to move, `Rejected` for a target that takes no registrations.
    async fn move_judge_registrations(
        &self,
        actor: &Actor,
        cmd: MoveJudgeRegistrations,
    ) -> Result<(), CommandError>;

    /// Registers the judge of an unmapped registration for a unit (or just
    /// drops it with `None`) and removes the unmapped entry.
    async fn resolve_unmapped_registration(
        &self,
        actor: &Actor,
        cmd: ResolveUnmappedRegistration,
    ) -> Result<(), CommandError>;

    /// Registers the judges of a remap file again, in one unit of work.
    async fn apply_jury_remap(
        &self,
        actor: &Actor,
        cmd: ApplyJuryRemap,
    ) -> Result<RemapOutcome, CommandError>;

    /// Credits a club with jury deployments instead of the registered ones;
    /// `None` uses the registered ones again. `Rejected` for a negative
    /// number, `NotFound` for an unknown club.
    async fn set_jury_deployments_override(
        &self,
        actor: &Actor,
        cmd: SetJuryDeploymentsOverride,
    ) -> Result<(), CommandError>;

    /// Publishes the Juryplan (at `now`) or takes it back. Publishing is
    /// `Rejected` (German message) before the end of the judge registration.
    async fn set_jury_plan_published(
        &self,
        actor: &Actor,
        cmd: SetJuryPlanPublished,
    ) -> Result<(), CommandError>;
}
