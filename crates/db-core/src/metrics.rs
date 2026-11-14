//! What the Prometheus metrics read from the database: [`MetricsRepo`].

use crate::DbError;

/// The counts of the database, read in one consistent snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbCounts {
    /// Users by `email_verified`.
    pub users: Vec<(bool, i64)>,
    pub clubs: i64,
    pub riders: i64,
    pub riders_checked_in: i64,
    /// Riders per club name.
    pub riders_by_club: Vec<(String, i64)>,
    /// Acts by act type and state (`withdrawn`, `categorized`,
    /// `uncategorized`).
    pub acts: Vec<(String, String, i64)>,
    /// Active acts per category.
    pub acts_by_category: Vec<(String, i64)>,
    /// Riders of groups as `(is_replacement, count)`.
    pub group_riders: Vec<(bool, i64)>,
    /// Songs by state (`missing`, `checked`, `uploaded`).
    pub songs: Vec<(String, i64)>,
    /// Acts by normalization status.
    pub song_normalization: Vec<(String, i64)>,
    pub acts_performed: i64,
    /// Timeplan entries by status (`ended`, `started`, `planned`).
    pub timeplan_entries: Vec<(String, i64)>,
}

#[async_trait::async_trait]
pub trait MetricsRepo {
    /// Counts registrations, acts, songs and the timeplan.
    async fn db_counts(&self) -> Result<DbCounts, DbError>;
}
