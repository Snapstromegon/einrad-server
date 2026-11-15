//! Club queries: clubs, riders, pairs, acts of a club, judges, mails.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::OffsetDateTime;

use crate::{
    act_type::ActType,
    actor::Actor,
    acts::NormalizationStatus,
    bookings::Payment,
    error::QueryError,
    fees::ClubFees,
    gender::Gender,
    ids::{ActId, ClubId, RiderId, UserId},
};

/// One club, with the details only the club itself and the organizer see.
#[derive(Debug, Clone)]
pub struct GetClub {
    pub club_id: ClubId,
}

/// The acts of one club.
#[derive(Debug, Clone)]
pub struct ListClubActs {
    pub club_id: ClubId,
}

/// A club with its owner and contact.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubProfile {
    pub id: ClubId,
    pub name: String,
    pub owner_id: UserId,
    pub fee_waived: bool,
    pub owner_name: Option<String>,
    pub owner_email: Option<String>,
    /// The club's own responsible person, else the owner.
    pub responsible_name: Option<String>,
    pub responsible_email: Option<String>,
}

/// What `get_club` shows about a club's money, only for who may see it.
#[derive(Debug, Clone)]
pub struct ClubDetails {
    /// Oldest first; refunds are negative.
    pub payments: Vec<Payment>,
    pub fees: ClubFees,
}

/// A club as `get_club` returns it.
#[derive(Debug, Clone)]
pub struct ClubView {
    pub profile: ClubProfile,
    /// `Some` only if the actor is the organizer or belongs to the club (see
    /// `Actor::may_see_club_details`). The handler hides the owner's and
    /// the responsible person's contact data when this is `None`.
    pub details: Option<ClubDetails>,
}

/// A row of the club list.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubRow {
    pub id: ClubId,
    pub name: String,
    pub owner_id: UserId,
    pub fee_waiver: Option<String>,
    pub owner_name: String,
    pub owner_email: String,
    pub owner_email_verified: bool,
    pub responsible_name: String,
    pub responsible_email: String,
}

/// A club of the club list with what it paid and owes. Which parts a caller
/// sees is the handler's call (role decision).
#[derive(Debug, Clone)]
pub struct ClubListEntry {
    pub club: ClubRow,
    /// The sum of the club's payments; 0 without payments.
    pub paid: f64,
    pub fees: ClubFees,
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
    pub created_at: OffsetDateTime,
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
    pub birthdate: OffsetDateTime,
}

/// The acts of a club, with the competition setting the handler needs to
/// decide what to show.
#[derive(Debug, Clone)]
pub struct ClubActs {
    /// Whether the competition lets clubs enter a description for the
    /// moderation; the handler shows descriptions only then.
    pub moderation_input: bool,
    pub acts: Vec<ClubActRecord>,
}

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

    /// One club. The details (payments, fees) come with it only for the
    /// organizer and the club's own users; `actor` is `None` for anonymous
    /// callers. `NotFound` for an unknown club. One read transaction.
    async fn get_club(&self, actor: Option<&Actor>, query: GetClub)
    -> Result<ClubView, QueryError>;

    /// All clubs by name, each with what it paid and owes. One read
    /// transaction.
    async fn list_clubs(&self, actor: &Actor) -> Result<Vec<ClubListEntry>, QueryError>;

    /// Every act with a rider of the club or created by it, with
    /// participants and unfiltered. `NotFound` for an unknown club,
    /// `Forbidden` unless the actor manages it.
    async fn list_club_acts(
        &self,
        actor: &Actor,
        query: ListClubActs,
    ) -> Result<ClubActs, QueryError>;
}
