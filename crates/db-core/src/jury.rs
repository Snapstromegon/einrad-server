//! The pure part of the jury obligation (the SQL that counts a club's acts
//! and judges lives in the backend).

/// The deployments a judge counts for their club: jury units with a
/// technical or performance registration, at most their maximum; untrained
/// people count none.
///
/// ```
/// use db_core::jury::counted_deployments;
///
/// assert_eq!(counted_deployments(true, 6, None), 6);
/// assert_eq!(counted_deployments(true, 6, Some(2)), 2);
/// assert_eq!(counted_deployments(false, 6, None), 0);
/// ```
#[must_use]
pub fn counted_deployments(is_trained: bool, registered: i64, max: Option<i64>) -> i64 {
    if !is_trained {
        return 0;
    }
    max.map_or(registered, |max| registered.min(max))
}

/// Jury deployments (Einsätze) a club must provide: `per_two_acts` for every
/// started two Küren (Jury-Konzept, Tabelle 1).
///
/// ```
/// use db_core::jury::required_deployments;
///
/// assert_eq!(required_deployments(0, 4), 0);
/// assert_eq!(required_deployments(1, 4), 4);
/// assert_eq!(required_deployments(2, 4), 4);
/// assert_eq!(required_deployments(3, 4), 8);
/// assert_eq!(required_deployments(32, 4), 64);
/// ```
#[must_use]
pub fn required_deployments(acts: i64, per_two_acts: i64) -> i64 {
    (acts + 1) / 2 * per_two_acts
}

/// A club's jury obligation.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, utoipa::ToSchema)]
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
