//! The rules of rider registration as pure functions (#238, ADR 0019): a
//! rider's single and pair starts, pair requests, replacing a rider in an act
//! and the once-per-club name rule.
//!
//! A backend loads the facts ([`RiderFacts`], [`Starts`], ...), asks one of
//! the `check_*` functions and writes what it decides. The German messages of
//! the rejections live here, so no backend repeats them.

use crate::{
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
    people,
    riders::{Pair, Starts},
};

/// What the rules need to know about a rider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiderFacts {
    pub id: RiderId,
    pub club_id: ClubId,
    /// First and last name, joined by a space.
    pub name: String,
    pub gender: Option<Gender>,
}

/// The name of a rider a club already registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiderName {
    pub firstname: String,
    pub lastname: String,
}

/// A rider of the club waiting for a pair partner (a pair request).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingRider {
    pub id: RiderId,
    /// First and last name, joined by a space.
    pub name: String,
    /// The partner the rider asked for, if they named one.
    pub partner_name: Option<String>,
}

/// Whose riders a pair can be linked with: clubs only link their own riders,
/// the organizer also riders of different clubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartnerClubs {
    Same,
    Any,
}

impl PartnerClubs {
    /// What the actor may link: the organizer (`manage_registrations`) any
    /// riders, everybody else their club's.
    #[must_use]
    pub fn of(actor: &Actor) -> Self {
        if actor.manage_registrations {
            Self::Any
        } else {
            Self::Same
        }
    }
}

fn rejected(message: impl Into<String>) -> RegistrationError {
    RegistrationError::Rejected(message.into())
}

/// A single needs the rider's gender: it's Einzel weiblich or männlich by it.
#[must_use]
pub fn gender_missing(rider: &RiderFacts) -> RegistrationError {
    rejected(format!(
        "Bitte gib zuerst das Geschlecht von {} an.",
        rider.name
    ))
}

#[must_use]
pub fn same_person() -> RegistrationError {
    rejected("Fahrer und Partner sind die selbe Person!")
}

/// The rider already is in a pair act.
#[must_use]
pub fn already_in_pair(rider: &RiderFacts) -> RegistrationError {
    rejected(format!("{} fährt schon eine Paarkür.", rider.name))
}

/// The rider already has a single act.
#[must_use]
pub fn already_has_single(rider: &RiderFacts) -> RegistrationError {
    rejected(format!("{} fährt schon eine Einzelkür.", rider.name))
}

/// The pair partner of a club's rider must be from that club.
#[must_use]
pub fn partner_of_other_club() -> RegistrationError {
    rejected(
        "Die Paarkür-PartnerIn muss aus dem eigenen Verein sein. Vereinsübergreifende Paare verbindet der Veranstalter.",
    )
}

/// Only pair acts are dissolved.
#[must_use]
pub fn not_a_pair() -> RegistrationError {
    rejected("Die Kür ist keine Paarkür.")
}

/// Groups choose their riders themselves.
#[must_use]
pub fn group_riders_are_chosen_in_the_group() -> RegistrationError {
    rejected("Die Fahrer einer Gruppe werden in der Gruppe ausgewählt.")
}

#[must_use]
pub fn act_withdrawn() -> RegistrationError {
    rejected("Die Kür ist abgemeldet und kann nicht mehr geändert werden.")
}

#[must_use]
pub fn replacement_of_other_club() -> RegistrationError {
    rejected("Der neue Fahrer muss aus demselben Verein sein.")
}

#[must_use]
pub fn already_rides_act() -> RegistrationError {
    rejected("Der Fahrer fährt diese Kür schon.")
}

/// Fails if the club already registered a rider with this name, giving the
/// name as registered (it may differ in case). `registered` are the club's
/// other riders (not the one being edited).
pub fn ensure_rider_name_is_free(
    registered: &[RiderName],
    firstname: &str,
    lastname: &str,
) -> Result<(), RegistrationError> {
    let key = people::person_key(firstname, lastname);
    match registered
        .iter()
        .find(|name| people::person_key(&name.firstname, &name.lastname) == key)
    {
        Some(name) => Err(rejected(people::already_registered_message(
            &name.firstname,
            &name.lastname,
        ))),
        None => Ok(()),
    }
}

/// What to do with a rider's single.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleOp {
    Nothing,
    /// Create a single act for the rider.
    Create,
    /// Delete the single act.
    Delete(ActId),
}

/// Adds or removes the rider's single. A new single needs the rider's gender.
/// `current` is the rider's single act, if any.
pub fn check_set_single(
    rider: &RiderFacts,
    current: Option<ActId>,
    single: bool,
) -> Result<SingleOp, RegistrationError> {
    match (single, current) {
        (true, None) if rider.gender.is_none() => Err(gender_missing(rider)),
        (true, None) => Ok(SingleOp::Create),
        (false, Some(act_id)) => Ok(SingleOp::Delete(act_id)),
        _ => Ok(SingleOp::Nothing),
    }
}

/// One pair per rider: fails if the rider is in a pair act.
pub fn ensure_not_in_pair(rider: &RiderFacts, starts: &Starts) -> Result<(), RegistrationError> {
    match starts.pair {
        Pair::Linked { .. } => Err(already_in_pair(rider)),
        Pair::None | Pair::Requested { .. } => Ok(()),
    }
}

/// A rider asks for a pair partner, named or not. Fails if the rider already
/// is in a pair or names themselves. Returns the partner name to store: no
/// surrounding spaces, `None` if empty.
pub fn check_request_pair(
    rider: &RiderFacts,
    starts: &Starts,
    partner_name: Option<&str>,
) -> Result<Option<String>, RegistrationError> {
    ensure_not_in_pair(rider, starts)?;
    let partner_name = partner_name.map(str::trim).filter(|name| !name.is_empty());
    if partner_name == Some(rider.name.as_str()) {
        return Err(same_person());
    }
    Ok(partner_name.map(str::to_string))
}

/// The waiting rider of the club who becomes the partner at once: the one
/// with the name the rider asked for, who names this rider (or nobody). There
/// has to be exactly one; with several, the club decides.
#[must_use]
pub fn pick_waiting_partner(
    rider: &RiderFacts,
    partner_name: &str,
    waiting: &[WaitingRider],
) -> Option<RiderId> {
    let mut candidates = waiting.iter().filter(|candidate| {
        candidate.id != rider.id
            && candidate.name == partner_name
            && candidate
                .partner_name
                .as_deref()
                .is_none_or(|name| name == rider.name)
    });
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first.id)
}

/// Links two riders to a pair act. Fails if they are the same person, a club
/// links a rider of another club, or one of them already is in a pair. The
/// facts are the riders' ([`RiderFacts`] and [`Starts`]).
pub fn check_link_pair(
    rider: (&RiderFacts, &Starts),
    partner: (&RiderFacts, &Starts),
    clubs: PartnerClubs,
) -> Result<(), RegistrationError> {
    if rider.0.id == partner.0.id {
        return Err(same_person());
    }
    if clubs == PartnerClubs::Same && rider.0.club_id != partner.0.club_id {
        return Err(partner_of_other_club());
    }
    ensure_not_in_pair(rider.0, rider.1)?;
    ensure_not_in_pair(partner.0, partner.1)
}

/// What to do when a rider leaves the pair competition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeavePairOp {
    Nothing,
    /// Delete the rider's pair request.
    DropRequest,
    /// Delete the pair act; the partner is left with a pair request without a
    /// name.
    Dissolve {
        act_id: ActId,
        requeue_partner: Option<RiderId>,
    },
}

#[must_use]
pub fn check_leave_pair(starts: &Starts) -> LeavePairOp {
    match &starts.pair {
        Pair::None => LeavePairOp::Nothing,
        Pair::Requested { .. } => LeavePairOp::DropRequest,
        Pair::Linked { act_id, partner } => LeavePairOp::Dissolve {
            act_id: *act_id,
            requeue_partner: *partner,
        },
    }
}

/// Dissolves a pair act: only pair acts are dissolved. `act_type` is `None`
/// for an unknown act.
pub fn check_dissolve_pair(act_type: Option<ActType>) -> Result<(), RegistrationError> {
    match act_type {
        None => Err(RegistrationError::NotFound),
        Some(ActType::Pair) => Ok(()),
        Some(_) => Err(not_a_pair()),
    }
}

/// An act can't be changed once it is withdrawn.
pub fn check_act_open(withdrawn: bool) -> Result<(), RegistrationError> {
    if withdrawn {
        Err(act_withdrawn())
    } else {
        Ok(())
    }
}

/// The new rider of an act is from the old rider's club, unless the actor is
/// the organizer.
pub fn check_replacement_club(
    actor: &Actor,
    old_club: ClubId,
    new_club: ClubId,
) -> Result<(), RegistrationError> {
    if old_club != new_club && !actor.manage_registrations {
        Err(replacement_of_other_club())
    } else {
        Ok(())
    }
}

/// `old` has to ride the act, `new` must not already.
pub fn check_act_participants(
    participants: &[RiderId],
    old: RiderId,
    new: RiderId,
) -> Result<(), RegistrationError> {
    if !participants.contains(&old) {
        return Err(RegistrationError::NotFound);
    }
    if participants.contains(&new) {
        return Err(already_rides_act());
    }
    Ok(())
}

/// What to do besides replacing the rider in the act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceOp {
    Replace,
    /// The pair act fulfils the new rider's pair request: delete it.
    ReplaceAndDropRequest,
}

/// Replaces a rider of a single or pair act by `new` (their facts and
/// starts). `new` must not have such an act yet; groups choose their riders
/// in the group.
pub fn check_replace_rider(
    act_type: ActType,
    new: &RiderFacts,
    new_starts: &Starts,
) -> Result<ReplaceOp, RegistrationError> {
    match act_type {
        ActType::Single => {
            if new_starts.single.is_some() {
                return Err(already_has_single(new));
            }
            if new.gender.is_none() {
                return Err(gender_missing(new));
            }
            Ok(ReplaceOp::Replace)
        }
        ActType::Pair => {
            ensure_not_in_pair(new, new_starts)?;
            Ok(ReplaceOp::ReplaceAndDropRequest)
        }
        ActType::GroupSmall | ActType::GroupLarge => Err(group_riders_are_chosen_in_the_group()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::UserId;

    fn rider(name: &str) -> RiderFacts {
        RiderFacts {
            id: RiderId::new(),
            club_id: ClubId::new(),
            name: name.to_string(),
            gender: Some(Gender::Female),
        }
    }

    fn in_club(club_id: ClubId, name: &str) -> RiderFacts {
        RiderFacts {
            club_id,
            ..rider(name)
        }
    }

    fn no_starts() -> Starts {
        Starts {
            single: None,
            pair: Pair::None,
        }
    }

    fn linked() -> Starts {
        Starts {
            single: None,
            pair: Pair::Linked {
                act_id: ActId::new(),
                partner: Some(RiderId::new()),
            },
        }
    }

    fn requested() -> Starts {
        Starts {
            single: None,
            pair: Pair::Requested { partner_name: None },
        }
    }

    fn message<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> String {
        match result.unwrap_err() {
            RegistrationError::Rejected(message) => message,
            other => panic!("not a message: {other:?}"),
        }
    }

    fn actor(manage_registrations: bool) -> Actor {
        Actor {
            user_id: UserId::new(),
            club_id: None,
            manage_registrations,
        }
    }

    #[test]
    fn names_are_free_unless_the_club_has_them_ignoring_case() {
        let registered = [RiderName {
            firstname: "Max".into(),
            lastname: "Müller".into(),
        }];
        assert!(ensure_rider_name_is_free(&[], "Max", "Müller").is_ok());
        assert!(ensure_rider_name_is_free(&registered, "Max", "Schmidt").is_ok());
        assert!(ensure_rider_name_is_free(&registered, "Moritz", "Müller").is_ok());
        assert_eq!(
            message(ensure_rider_name_is_free(&registered, " max ", "MÜLLER")),
            "Max Müller ist bereits gemeldet."
        );
    }

    #[test]
    fn a_single_is_added_removed_or_kept() {
        let anna = rider("Anna Muster");
        let act = ActId::new();
        assert_eq!(
            check_set_single(&anna, None, true).unwrap(),
            SingleOp::Create
        );
        assert_eq!(
            check_set_single(&anna, Some(act), true).unwrap(),
            SingleOp::Nothing
        );
        assert_eq!(
            check_set_single(&anna, Some(act), false).unwrap(),
            SingleOp::Delete(act)
        );
        assert_eq!(
            check_set_single(&anna, None, false).unwrap(),
            SingleOp::Nothing
        );
    }

    #[test]
    fn a_new_single_needs_the_gender_but_removing_one_doesnt() {
        let anna = RiderFacts {
            gender: None,
            ..rider("Anna Muster")
        };
        assert_eq!(
            message(check_set_single(&anna, None, true)),
            "Bitte gib zuerst das Geschlecht von Anna Muster an."
        );
        // An existing single stays, whatever the gender.
        let act = ActId::new();
        assert_eq!(
            check_set_single(&anna, Some(act), true).unwrap(),
            SingleOp::Nothing
        );
        assert_eq!(
            check_set_single(&anna, Some(act), false).unwrap(),
            SingleOp::Delete(act)
        );
        assert_eq!(
            check_set_single(&anna, None, false).unwrap(),
            SingleOp::Nothing
        );
    }

    #[test]
    fn a_pair_request_trims_and_drops_empty_names() {
        let anna = rider("Anna Muster");
        for (given, stored) in [
            (None, None),
            (Some(""), None),
            (Some("   "), None),
            (Some(" Berta Muster "), Some("Berta Muster")),
        ] {
            assert_eq!(
                check_request_pair(&anna, &no_starts(), given).unwrap(),
                stored.map(str::to_string)
            );
        }
    }

    #[test]
    fn a_pair_request_may_be_changed_but_not_made_from_a_pair() {
        let anna = rider("Anna Muster");
        assert!(check_request_pair(&anna, &requested(), Some("Berta")).is_ok());
        assert_eq!(
            message(check_request_pair(&anna, &linked(), None)),
            "Anna Muster fährt schon eine Paarkür."
        );
    }

    #[test]
    fn a_pair_request_cant_name_the_rider_themselves() {
        let anna = rider("Anna Muster");
        assert_eq!(
            message(check_request_pair(
                &anna,
                &no_starts(),
                Some(" Anna Muster ")
            )),
            "Fahrer und Partner sind die selbe Person!"
        );
        // Being in a pair is reported first.
        assert!(
            message(check_request_pair(&anna, &linked(), Some("Anna Muster")))
                .contains("schon eine Paarkür")
        );
        // Only the exact name counts.
        assert!(check_request_pair(&anna, &no_starts(), Some("anna muster")).is_ok());
    }

    fn waiting(name: &str, partner_name: Option<&str>) -> WaitingRider {
        WaitingRider {
            id: RiderId::new(),
            name: name.to_string(),
            partner_name: partner_name.map(str::to_string),
        }
    }

    #[test]
    fn a_waiting_rider_naming_this_rider_or_nobody_is_the_partner() {
        let berta = rider("Berta Muster");
        let anna = waiting("Anna Muster", Some("Berta Muster"));
        let nobody = waiting("Anna Muster", None);
        assert_eq!(
            pick_waiting_partner(&berta, "Anna Muster", std::slice::from_ref(&anna)),
            Some(anna.id)
        );
        assert_eq!(
            pick_waiting_partner(&berta, "Anna Muster", std::slice::from_ref(&nobody)),
            Some(nobody.id)
        );
    }

    #[test]
    fn nobody_is_taken_who_names_someone_else_or_has_another_name() {
        let berta = rider("Berta Muster");
        assert_eq!(
            pick_waiting_partner(
                &berta,
                "Anna Muster",
                &[waiting("Anna Muster", Some("Clara Muster"))]
            ),
            None
        );
        assert_eq!(
            pick_waiting_partner(&berta, "Anna Muster", &[waiting("Zoe Muster", None)]),
            None
        );
        assert_eq!(pick_waiting_partner(&berta, "Anna Muster", &[]), None);
    }

    #[test]
    fn nobody_is_taken_when_several_wait_under_that_name() {
        let berta = rider("Berta Muster");
        assert_eq!(
            pick_waiting_partner(
                &berta,
                "Anna Muster",
                &[waiting("Anna Muster", None), waiting("Anna Muster", None)]
            ),
            None
        );
    }

    #[test]
    fn the_rider_isnt_their_own_partner() {
        let anna = rider("Anna Muster");
        let own = WaitingRider {
            id: anna.id,
            name: "Anna Muster".into(),
            partner_name: None,
        };
        assert_eq!(pick_waiting_partner(&anna, "Anna Muster", &[own]), None);
    }

    #[test]
    fn a_pair_links_riders_without_a_pair() {
        let club = ClubId::new();
        let (anna, berta) = (in_club(club, "Anna Muster"), in_club(club, "Berta Muster"));
        for starts in [no_starts(), requested()] {
            assert!(
                check_link_pair((&anna, &starts), (&berta, &no_starts()), PartnerClubs::Same)
                    .is_ok()
            );
        }
    }

    #[test]
    fn a_pair_needs_two_people() {
        let anna = rider("Anna Muster");
        for clubs in [PartnerClubs::Same, PartnerClubs::Any] {
            assert_eq!(
                message(check_link_pair(
                    (&anna, &no_starts()),
                    (&anna, &no_starts()),
                    clubs
                )),
                "Fahrer und Partner sind die selbe Person!"
            );
        }
    }

    #[test]
    fn clubs_link_their_own_riders_the_organizer_any() {
        let (anna, zoe) = (rider("Anna Muster"), rider("Zoe Muster"));
        assert!(
            message(check_link_pair(
                (&anna, &no_starts()),
                (&zoe, &no_starts()),
                PartnerClubs::Same
            ))
            .contains("eigenen Verein")
        );
        assert!(
            check_link_pair(
                (&anna, &no_starts()),
                (&zoe, &no_starts()),
                PartnerClubs::Any
            )
            .is_ok()
        );
    }

    #[test]
    fn a_rider_in_a_pair_cant_be_linked_again() {
        let club = ClubId::new();
        let (anna, berta) = (in_club(club, "Anna Muster"), in_club(club, "Berta Muster"));
        assert_eq!(
            message(check_link_pair(
                (&anna, &linked()),
                (&berta, &no_starts()),
                PartnerClubs::Same
            )),
            "Anna Muster fährt schon eine Paarkür."
        );
        assert_eq!(
            message(check_link_pair(
                (&anna, &no_starts()),
                (&berta, &linked()),
                PartnerClubs::Same
            )),
            "Berta Muster fährt schon eine Paarkür."
        );
    }

    #[test]
    fn partner_clubs_follow_the_actor() {
        assert_eq!(PartnerClubs::of(&actor(true)), PartnerClubs::Any);
        assert_eq!(PartnerClubs::of(&actor(false)), PartnerClubs::Same);
    }

    #[test]
    fn leaving_depends_on_where_the_rider_stands() {
        assert_eq!(check_leave_pair(&no_starts()), LeavePairOp::Nothing);
        assert_eq!(check_leave_pair(&requested()), LeavePairOp::DropRequest);
        let (act_id, partner) = (ActId::new(), RiderId::new());
        let pair = |partner| Starts {
            single: Some(ActId::new()),
            pair: Pair::Linked { act_id, partner },
        };
        assert_eq!(
            check_leave_pair(&pair(Some(partner))),
            LeavePairOp::Dissolve {
                act_id,
                requeue_partner: Some(partner)
            }
        );
        assert_eq!(
            check_leave_pair(&pair(None)),
            LeavePairOp::Dissolve {
                act_id,
                requeue_partner: None
            }
        );
    }

    #[test]
    fn only_pair_acts_are_dissolved() {
        assert!(check_dissolve_pair(Some(ActType::Pair)).is_ok());
        assert!(matches!(
            check_dissolve_pair(None),
            Err(RegistrationError::NotFound)
        ));
        for other in [ActType::Single, ActType::GroupSmall, ActType::GroupLarge] {
            assert_eq!(
                message(check_dissolve_pair(Some(other))),
                "Die Kür ist keine Paarkür."
            );
        }
    }

    #[test]
    fn a_withdrawn_act_is_closed() {
        assert!(check_act_open(false).is_ok());
        assert_eq!(
            message(check_act_open(true)),
            "Die Kür ist abgemeldet und kann nicht mehr geändert werden."
        );
    }

    #[test]
    fn the_replacement_is_from_the_same_club_unless_the_organizer_replaces() {
        let (a, b) = (ClubId::new(), ClubId::new());
        assert!(check_replacement_club(&actor(false), a, a).is_ok());
        assert!(check_replacement_club(&actor(true), a, b).is_ok());
        assert_eq!(
            message(check_replacement_club(&actor(false), a, b)),
            "Der neue Fahrer muss aus demselben Verein sein."
        );
    }

    #[test]
    fn the_old_rider_rides_the_act_and_the_new_one_doesnt() {
        let (old, new, other) = (RiderId::new(), RiderId::new(), RiderId::new());
        assert!(check_act_participants(&[old, other], old, new).is_ok());
        assert!(matches!(
            check_act_participants(&[other], old, new),
            Err(RegistrationError::NotFound)
        ));
        // An unknown old rider is reported before a known new one.
        assert!(matches!(
            check_act_participants(&[new], old, new),
            Err(RegistrationError::NotFound)
        ));
        assert_eq!(
            message(check_act_participants(&[old, new], old, new)),
            "Der Fahrer fährt diese Kür schon."
        );
    }

    #[test]
    fn a_single_goes_to_a_rider_without_single_and_with_gender() {
        let new = rider("Berta Muster");
        assert_eq!(
            check_replace_rider(ActType::Single, &new, &requested()).unwrap(),
            ReplaceOp::Replace
        );
        // A pair doesn't matter for a single.
        assert_eq!(
            check_replace_rider(ActType::Single, &new, &linked()).unwrap(),
            ReplaceOp::Replace
        );
        let has_single = Starts {
            single: Some(ActId::new()),
            pair: Pair::None,
        };
        assert_eq!(
            message(check_replace_rider(ActType::Single, &new, &has_single)),
            "Berta Muster fährt schon eine Einzelkür."
        );
        let no_gender = RiderFacts {
            gender: None,
            ..new
        };
        assert_eq!(
            message(check_replace_rider(
                ActType::Single,
                &no_gender,
                &no_starts()
            )),
            "Bitte gib zuerst das Geschlecht von Berta Muster an."
        );
        // The single is reported before the missing gender.
        assert!(
            message(check_replace_rider(
                ActType::Single,
                &no_gender,
                &has_single
            ))
            .contains("Einzelkür")
        );
    }

    #[test]
    fn a_pair_goes_to_a_rider_without_pair_and_fulfils_their_request() {
        let new = rider("Berta Muster");
        for starts in [no_starts(), requested()] {
            assert_eq!(
                check_replace_rider(ActType::Pair, &new, &starts).unwrap(),
                ReplaceOp::ReplaceAndDropRequest
            );
        }
        assert_eq!(
            message(check_replace_rider(ActType::Pair, &new, &linked())),
            "Berta Muster fährt schon eine Paarkür."
        );
        // The gender isn't needed for a pair.
        let no_gender = RiderFacts {
            gender: None,
            ..new
        };
        assert!(check_replace_rider(ActType::Pair, &no_gender, &no_starts()).is_ok());
    }

    #[test]
    fn groups_choose_their_riders_themselves() {
        let new = rider("Berta Muster");
        for act_type in [ActType::GroupSmall, ActType::GroupLarge] {
            assert_eq!(
                message(check_replace_rider(act_type, &new, &no_starts())),
                "Die Fahrer einer Gruppe werden in der Gruppe ausgewählt."
            );
        }
    }
}
