//! Riders, their singles and pairs: the [`RidersRepo`] trait and its types.
//!
//! A rider's starts are read from acts and act participants only; a rider
//! waiting for a pair partner who isn't linked yet has a pair request instead
//! (ADR 0004). Every change of singles, pairs and pair requests goes through
//! here, so a rider has at most one single and either one pair act or one
//! pair request. Groups are set in [`crate::groups`].

use time::OffsetDateTime;

use crate::{
    DbError,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
};

/// A rider's single and pair starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Starts {
    /// The rider's single act.
    pub single: Option<ActId>,
    pub pair: Pair,
}

/// Where a rider stands in the pair competition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    None,
    /// Waiting for a partner (Paaranfrage): the name the club entered, if any.
    Requested {
        partner_name: Option<String>,
    },
    /// In a pair act with `partner`.
    Linked {
        act_id: ActId,
        partner: Option<RiderId>,
    },
}

/// A new rider; names are trimmed.
#[derive(Debug, Clone)]
pub struct NewRider {
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    pub gender: Gender,
}

/// Changes to a rider; names are trimmed. A missing gender stays as it is.
#[derive(Debug, Clone)]
pub struct RiderEdit {
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    pub gender: Option<Gender>,
}

/// A rider with their club and starts.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Rider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    /// Only for who may see birthdates.
    #[serde(with = "time::serde::iso8601::option")]
    pub birthdate: Option<OffsetDateTime>,
    pub gender: Option<Gender>,
    /// On the determining day; only for who may see birthdates.
    pub age_on_competition: Option<f64>,
    /// When the rider checked in on competition day.
    #[serde(with = "time::serde::iso8601::option")]
    pub checked_in_at: Option<OffsetDateTime>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    pub group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    pub waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    pub fee_waived: bool,
    /// Why the rider's fees are waived.
    pub fee_waiver: Option<String>,
}

/// A rider as their own club sees them.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubRider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: OffsetDateTime,
    /// Decides whether the single is Einzel weiblich or männlich. Missing for
    /// riders registered before it was asked.
    pub gender: Option<Gender>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    pub group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    pub waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    pub fee_waived: bool,
}

/// A rider waiting for a pair partner who isn't linked yet (Paaranfrage).
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct PairRequest {
    pub rider_id: RiderId,
    /// The partner the club entered, if any.
    pub partner_name: Option<String>,
}

#[async_trait::async_trait]
pub trait RidersRepo {
    /// Registers a rider with the club. The club registers each name once
    /// (ignoring case and spaces).
    async fn add_rider(
        &self,
        actor: &Actor,
        rider: &NewRider,
    ) -> Result<RiderId, RegistrationError>;

    async fn edit_rider(
        &self,
        actor: &Actor,
        rider: RiderId,
        edit: &RiderEdit,
    ) -> Result<(), RegistrationError>;

    /// Deletes the rider with their single; their pair is dissolved (the
    /// partner is left waiting) and their groups keep the other riders.
    async fn delete_rider(&self, actor: &Actor, rider: RiderId) -> Result<(), RegistrationError>;

    /// Every rider with their club and starts.
    async fn list_riders(&self) -> Result<Vec<Rider>, DbError>;

    async fn list_club_riders(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<ClubRider>, RegistrationError>;

    /// The pair requests of all clubs.
    async fn list_pair_requests(&self) -> Result<Vec<PairRequest>, DbError>;

    async fn list_club_pair_requests(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<PairRequest>, RegistrationError>;

    /// The rider's single and pair starts.
    async fn rider_starts(&self, rider: RiderId) -> Result<Starts, DbError>;

    /// Adds or removes the rider's single. A new single needs the rider's
    /// gender.
    async fn set_single(
        &self,
        actor: &Actor,
        rider: RiderId,
        single: bool,
    ) -> Result<(), RegistrationError>;

    /// The rider waits for a pair partner, named or not. Within the club, a
    /// waiting rider of that name who names this rider (or nobody) becomes
    /// the partner at once. Returns that partner.
    async fn request_pair(
        &self,
        actor: &Actor,
        rider: RiderId,
        partner_name: Option<&str>,
    ) -> Result<Option<RiderId>, RegistrationError>;

    /// Links two riders to a pair act; their pair requests go. Clubs only
    /// link their own riders, the organizer (`manage_registrations`) also
    /// riders of different clubs.
    async fn link_pair(
        &self,
        actor: &Actor,
        rider: RiderId,
        partner: RiderId,
    ) -> Result<ActId, RegistrationError>;

    /// Takes the rider out of the pair competition: their pair request goes,
    /// or their pair act is dissolved and the partner is left with a pair
    /// request without a name.
    async fn leave_pair(&self, actor: &Actor, rider: RiderId) -> Result<(), RegistrationError>;

    /// Dissolves a pair act: both riders leave the pair competition, nobody
    /// is left waiting.
    async fn dissolve_pair(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError>;

    /// Replaces a rider of a single or pair act by `new`, keeping the act with
    /// its title, description and music.
    async fn replace_act_rider(
        &self,
        actor: &Actor,
        act: ActId,
        old: RiderId,
        new: RiderId,
    ) -> Result<(), RegistrationError>;
}
