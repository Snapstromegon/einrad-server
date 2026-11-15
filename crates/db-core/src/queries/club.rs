//! Club queries: clubs, riders, pairs, acts of a club, judges, mails.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::OffsetDateTime;

use crate::{
    actor::Actor,
    error::QueryError,
    gender::Gender,
    ids::{ClubId, RiderId},
};

/// The riders of one club.
#[derive(Debug, Clone)]
pub struct ListClubRiders {
    pub club_id: ClubId,
}

/// The pair requests of one club.
#[derive(Debug, Clone)]
pub struct ListClubPairRequests {
    pub club_id: ClubId,
}

/// A rider with their club and starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Rider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    /// Only for who may see birthdates.
    pub birthdate: Option<OffsetDateTime>,
    pub gender: Option<Gender>,
    /// On the determining day; only for who may see birthdates.
    pub age_on_competition: Option<f64>,
    /// When the rider checked in on competition day.
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
#[derive(Debug, Clone, PartialEq)]
pub struct ClubRider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
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
#[derive(Debug, Clone, PartialEq)]
pub struct PairRequest {
    pub rider_id: RiderId,
    /// The partner the club entered, if any.
    pub partner_name: Option<String>,
}

/// The read side of the club area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait ClubQueries: Send + Sync {
    /// Every rider with their club and starts. Who may see birthdates is the
    /// handler's call.
    async fn list_riders(&self, actor: &Actor) -> Result<Vec<Rider>, QueryError>;

    async fn list_club_riders(
        &self,
        actor: &Actor,
        query: ListClubRiders,
    ) -> Result<Vec<ClubRider>, QueryError>;

    /// The pair requests of all clubs.
    async fn list_pair_requests(&self, actor: &Actor) -> Result<Vec<PairRequest>, QueryError>;

    async fn list_club_pair_requests(
        &self,
        actor: &Actor,
        query: ListClubPairRequests,
    ) -> Result<Vec<PairRequest>, QueryError>;
}
