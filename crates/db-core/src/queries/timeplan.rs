//! Timeplan queries: the timeplan and the live schedule.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use time::OffsetDateTime;

use crate::{
    actor::Actor,
    acts::StartlistAct,
    error::QueryError,
    ids::{CategoryId, FinalId},
    timeplan::{Audience, EntryKind, Timeplan},
};

/// The entries in order, without their content.
#[derive(Debug, Clone)]
pub struct ListTimeplan {
    pub audience: Audience,
}

/// The whole timeplan with every block's content.
#[derive(Debug, Clone)]
pub struct PredictTimeplan {
    pub audience: Audience,
}

/// A timeplan entry as the list shows it, without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeplanListEntry {
    pub id: i64,
    pub kind: EntryKind,
    pub earliest_start_time: Option<OffsetDateTime>,
    pub duration_seconds: Option<i64>,
    pub label: Option<String>,
    /// Start blocks only: the home category, by id and name.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Start blocks only: the final running here instead of a category, by
    /// id and name.
    pub final_id: Option<FinalId>,
    pub final_name: Option<String>,
    /// Start blocks only: overrides the home category's (or final's)
    /// Einfahrzeit.
    pub einfahrzeit_seconds: Option<i64>,
    pub show_photos: bool,
    pub internal: bool,
    /// Start blocks only: the order was edited by hand.
    pub manually_ordered: bool,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
}

/// What the startlist CSV is made of: the public timeplan and the acts'
/// details, read in one snapshot.
#[derive(Debug)]
pub struct StartlistCsv {
    pub timeplan: Timeplan,
    pub acts: Vec<StartlistAct>,
}

/// The read side of the timeplan area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait TimeplanQueries: Send + Sync {
    /// The entries in order, as `audience` may see them.
    async fn list_timeplan(
        &self,
        actor: Option<&Actor>,
        query: ListTimeplan,
    ) -> Result<Vec<TimeplanListEntry>, QueryError>;

    /// The whole timeplan with every block's content, read consistently, as
    /// `audience` may see it. The handler predicts the times from it.
    async fn predict_timeplan(
        &self,
        actor: Option<&Actor>,
        query: PredictTimeplan,
    ) -> Result<Timeplan, QueryError>;

    /// The public timeplan with the details of the acts, in one read
    /// transaction.
    async fn get_startlist_csv(&self, actor: Option<&Actor>) -> Result<StartlistCsv, QueryError>;
}
