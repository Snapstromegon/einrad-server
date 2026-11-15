//! What clubs owe: start fees and the fee for missing jury deployments.
//!
//! The one place the fee rules live in the backend; the club queries, the
//! payment QR code (through them) and `/metrics` all use it.

use std::collections::{HashMap, HashSet};

use crate::{
    DbError,
    ids::ClubId,
    jury::{JuryFacts, JuryFeeRules, JuryObligation, jury_obligations},
};

/// What a club owes, split into its parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
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

/// What a club owes and, with the jury registration, its jury obligation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClubFees {
    pub amount_due: AmountDue,
    pub jury: Option<JuryObligation>,
}

/// The fee rules of the current competition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeeRules {
    pub fee_per_start: f64,
    pub max_fee_per_rider: Option<f64>,
    /// Only with the jury rules configured.
    pub jury: Option<JuryFeeRules>,
}

impl FeeRules {
    /// The jury fee only applies while the jury registration is on and the
    /// competition has jury rules.
    #[must_use]
    pub fn jury_rules(&self, jury_registration: bool) -> Option<&JuryFeeRules> {
        self.jury.as_ref().filter(|_| jury_registration)
    }
}

/// A rider as the start fees see them: the backend only counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiderFeeFacts {
    pub club_id: ClubId,
    /// The rider has a fee waiver.
    pub waived: bool,
    /// The rider's club has a fee waiver.
    pub club_waived: bool,
    /// The acts the rider is in.
    pub participations: i64,
    /// The rider waits for a pair partner.
    pub pair_request: bool,
    /// Of those acts, how many have a fee waiver.
    pub waived_participations: i64,
}

impl RiderFeeFacts {
    /// The starts the rider pays for. Each start in an act counts, and a pair
    /// request counts as one. With `waivers`, a waived club or rider pays
    /// nothing and a waived act's start doesn't count.
    #[must_use]
    pub fn starts(&self, waivers: bool) -> i64 {
        if waivers && (self.waived || self.club_waived) {
            return 0;
        }
        let waived_acts = if waivers {
            self.waived_participations
        } else {
            0
        };
        (self.participations + i64::from(self.pair_request) - waived_acts).max(0)
    }

    /// What the rider pays: `fee_per_start` per start, at most the cap per
    /// rider. The same rule as `getRiderPrice` in the frontend, which still
    /// prices single riders in the forms.
    #[must_use]
    pub fn fee(&self, rules: &FeeRules, waivers: bool) -> f64 {
        let fee = self.starts(waivers) as f64 * rules.fee_per_start;
        rules.max_fee_per_rider.map_or(fee, |max| fee.min(max))
    }
}

/// Start fees per club; the rider's club pays. Clubs without riders are
/// missing.
#[must_use]
pub fn start_fees_by_club(
    rules: &FeeRules,
    riders: &[RiderFeeFacts],
    waivers: bool,
) -> HashMap<ClubId, f64> {
    let mut fees = HashMap::new();
    for rider in riders {
        *fees.entry(rider.club_id).or_insert(0.0) += rider.fee(rules, waivers);
    }
    fees
}

/// What the backend loads to work out fees: counts and rows, no decisions.
#[derive(Debug, Clone, Default)]
pub struct FeeFacts {
    /// `None` without a competition: nothing is owed.
    pub rules: Option<FeeRules>,
    pub riders: Vec<RiderFeeFacts>,
    /// Clubs with a fee waiver.
    pub waived_clubs: HashSet<ClubId>,
    /// Only needed when the jury fee applies (see [`FeeRules::jury_rules`]).
    pub jury: JuryFacts,
}

/// The fees per club. The jury fee only applies while `jury_registration` is
/// on and the competition has jury rules. Clubs without riders and jury
/// obligation owe nothing and are missing from the map.
#[must_use]
pub fn club_fees(facts: &FeeFacts, jury_registration: bool) -> HashMap<ClubId, ClubFees> {
    let (start_fees, unwaived_start_fees) = match &facts.rules {
        Some(rules) => (
            start_fees_by_club(rules, &facts.riders, true),
            start_fees_by_club(rules, &facts.riders, false),
        ),
        None => Default::default(),
    };
    let obligations = match facts
        .rules
        .as_ref()
        .and_then(|rules| rules.jury_rules(jury_registration))
    {
        Some(rules) => jury_obligations(rules, &facts.jury),
        None => HashMap::new(),
    };
    let clubs: HashSet<ClubId> = unwaived_start_fees
        .keys()
        .chain(obligations.keys())
        .copied()
        .collect();
    clubs
        .into_iter()
        .map(|id| {
            let start_fees = start_fees.get(&id).copied().unwrap_or(0.0);
            let unwaived = unwaived_start_fees.get(&id).copied().unwrap_or(0.0);
            let jury = obligations.get(&id).copied();
            // A waived club keeps its obligation, only the fee is gone.
            let obligation_fee = jury.map_or(0.0, |jury| jury.fee);
            let jury_fee = if facts.waived_clubs.contains(&id) {
                0.0
            } else {
                obligation_fee
            };
            let fees = ClubFees {
                amount_due: AmountDue {
                    start_fees,
                    jury_fee,
                    total: start_fees + jury_fee,
                    waived: unwaived - start_fees + obligation_fee - jury_fee,
                },
                jury,
            };
            (id, fees)
        })
        .collect()
}

#[async_trait::async_trait]
pub trait FeesRepo {
    /// The fees of every club under the current competition's fee rules.
    /// Clubs without riders and jury obligation owe nothing and may be
    /// missing from the map. The jury fee only applies while
    /// `jury_registration` is on and the competition has jury rules.
    async fn fees_by_club(
        &self,
        jury_registration: bool,
    ) -> Result<HashMap<ClubId, ClubFees>, DbError>;

    /// The fees of one club.
    async fn fees_of_club(
        &self,
        jury_registration: bool,
        club: ClubId,
    ) -> Result<ClubFees, DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jury::JuryClubFacts;

    const RULES: FeeRules = FeeRules {
        fee_per_start: 10.0,
        max_fee_per_rider: None,
        jury: None,
    };

    fn rider(club: ClubId, participations: i64) -> RiderFeeFacts {
        RiderFeeFacts {
            club_id: club,
            waived: false,
            club_waived: false,
            participations,
            pair_request: false,
            waived_participations: 0,
        }
    }

    #[test]
    fn each_start_and_a_pair_request_cost_a_fee() {
        let c = ClubId::new();
        assert_eq!(rider(c, 0).fee(&RULES, true), 0.0);
        assert_eq!(rider(c, 3).fee(&RULES, true), 30.0);
        let waiting = RiderFeeFacts {
            pair_request: true,
            ..rider(c, 1)
        };
        assert_eq!(waiting.starts(true), 2);
        assert_eq!(
            RiderFeeFacts {
                pair_request: true,
                ..rider(c, 0)
            }
            .starts(true),
            1
        );
    }

    #[test]
    fn the_cap_applies_per_rider() {
        let c = ClubId::new();
        let rules = FeeRules {
            max_fee_per_rider: Some(25.0),
            ..RULES
        };
        assert_eq!(rider(c, 2).fee(&rules, true), 20.0);
        assert_eq!(rider(c, 3).fee(&rules, true), 25.0);
        // Two capped riders pay two caps.
        let fees = start_fees_by_club(&rules, &[rider(c, 5), rider(c, 5)], true);
        assert_eq!(fees[&c], 50.0);
    }

    #[test]
    fn waivers_drop_starts_only_when_applied() {
        let c = ClubId::new();
        let waived_rider = RiderFeeFacts {
            waived: true,
            ..rider(c, 2)
        };
        assert_eq!(waived_rider.starts(true), 0);
        assert_eq!(waived_rider.starts(false), 2);
        let waived_club = RiderFeeFacts {
            club_waived: true,
            ..rider(c, 2)
        };
        assert_eq!(waived_club.starts(true), 0);
        // Waived acts drop their start, never below 0.
        let some = RiderFeeFacts {
            waived_participations: 1,
            ..rider(c, 3)
        };
        assert_eq!((some.starts(true), some.starts(false)), (2, 3));
        let all = RiderFeeFacts {
            waived_participations: 5,
            ..rider(c, 3)
        };
        assert_eq!(all.starts(true), 0);
    }

    #[test]
    fn without_a_competition_nothing_is_owed() {
        let c = ClubId::new();
        let facts = FeeFacts {
            riders: vec![rider(c, 2)],
            ..Default::default()
        };
        assert!(club_fees(&facts, true).is_empty());
    }

    #[test]
    fn amount_due_splits_start_fees_and_what_waivers_drop() {
        let (a, b, c) = (ClubId::new(), ClubId::new(), ClubId::new());
        let facts = FeeFacts {
            rules: Some(RULES),
            riders: vec![
                rider(a, 2),
                RiderFeeFacts {
                    waived: true,
                    ..rider(a, 1)
                },
                RiderFeeFacts {
                    club_waived: true,
                    ..rider(b, 3)
                },
                rider(c, 0),
            ],
            waived_clubs: [b].into(),
            jury: JuryFacts::default(),
        };
        let fees = club_fees(&facts, false);
        let due = |id: ClubId| fees[&id].amount_due;
        assert_eq!(
            due(a),
            AmountDue {
                start_fees: 20.0,
                jury_fee: 0.0,
                total: 20.0,
                waived: 10.0
            }
        );
        assert_eq!(
            due(b),
            AmountDue {
                start_fees: 0.0,
                jury_fee: 0.0,
                total: 0.0,
                waived: 30.0
            }
        );
        // A club whose riders start nowhere still has an (empty) entry.
        assert_eq!(due(c), AmountDue::default());
        assert!(fees.values().all(|f| f.jury.is_none()));
    }

    fn jury_facts(club: ClubId, acts: usize) -> JuryFacts {
        use crate::{act_type::ActType, jury::JuryActFacts};
        JuryFacts {
            clubs: vec![JuryClubFacts {
                club_id: club,
                deployments_override: None,
            }],
            acts: (0..acts)
                .map(|_| JuryActFacts {
                    withdrawn: false,
                    fee_waived: false,
                    has_category: true,
                    act_type: ActType::Single,
                    regular_count: 1,
                    owner_club: None,
                    participants: vec![crate::jury::JuryParticipantFacts {
                        club_id: club,
                        is_replacement: false,
                    }],
                })
                .collect(),
            judges: vec![],
        }
    }

    #[test]
    fn the_jury_fee_needs_the_registration_and_the_rules() {
        let c = ClubId::new();
        let rules = FeeRules {
            jury: Some(JuryFeeRules {
                deployments_per_two_acts: 4,
                missing_deployment_fee: 12.5,
            }),
            ..RULES
        };
        let facts = FeeFacts {
            rules: Some(rules),
            riders: vec![rider(c, 2)],
            waived_clubs: HashSet::new(),
            jury: jury_facts(c, 2),
        };
        let on = club_fees(&facts, true)[&c];
        assert_eq!(on.amount_due.jury_fee, 50.0);
        assert_eq!(on.amount_due.total, 70.0);
        assert_eq!(on.jury.unwrap().missing, 4);
        let off = club_fees(&facts, false)[&c];
        assert_eq!(off.amount_due.jury_fee, 0.0);
        assert!(off.jury.is_none());
        let no_rules = FeeFacts {
            rules: Some(RULES),
            ..facts.clone()
        };
        assert!(club_fees(&no_rules, true)[&c].jury.is_none());
    }

    #[test]
    fn a_waived_club_keeps_its_obligation_but_not_the_fee() {
        let c = ClubId::new();
        let rules = FeeRules {
            jury: Some(JuryFeeRules {
                deployments_per_two_acts: 4,
                missing_deployment_fee: 10.0,
            }),
            ..RULES
        };
        let facts = FeeFacts {
            rules: Some(rules),
            riders: vec![RiderFeeFacts {
                club_waived: true,
                ..rider(c, 1)
            }],
            waived_clubs: [c].into(),
            jury: jury_facts(c, 1),
        };
        let fees = club_fees(&facts, true)[&c];
        assert_eq!(fees.jury.unwrap().fee, 40.0);
        assert_eq!(
            fees.amount_due,
            AmountDue {
                start_fees: 0.0,
                jury_fee: 0.0,
                total: 0.0,
                waived: 50.0
            }
        );
    }

    #[test]
    fn a_club_with_only_an_obligation_is_listed() {
        let c = ClubId::new();
        let rules = FeeRules {
            jury: Some(JuryFeeRules {
                deployments_per_two_acts: 2,
                missing_deployment_fee: 5.0,
            }),
            ..RULES
        };
        let facts = FeeFacts {
            rules: Some(rules),
            riders: vec![],
            waived_clubs: HashSet::new(),
            jury: jury_facts(c, 1),
        };
        assert_eq!(club_fees(&facts, true)[&c].amount_due.total, 10.0);
    }
}
