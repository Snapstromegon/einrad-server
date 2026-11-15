//! Clubs: the [`ClubsRepo`] trait, the [`Actor`] who asks and the
//! [`RegistrationError`] the club-side commands fail with.

use crate::{
    DbError,
    ids::{ActId, ClubId, UserId},
};

/// The one [`Actor`](crate::actor::Actor); kept under this path during the
/// CQRS migration (#238).
pub use crate::actor::Actor;

/// A club as far as access decisions go: what the backend loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClubFacts {
    pub id: ClubId,
    pub owner_id: UserId,
}

/// What the backend loads about an act to decide who may change it: the clubs
/// of its riders and, for a group, the club that created it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActFacts {
    pub clubs: Vec<ClubFacts>,
}

/// The outcome of an access decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    /// The actor may not (403).
    Forbidden,
    /// The thing doesn't exist (404).
    NotFound,
}

impl Access {
    /// `Ok` when allowed, else the matching [`RegistrationError`].
    pub fn into_result(self) -> Result<(), RegistrationError> {
        match self {
            Self::Allowed => Ok(()),
            Self::Forbidden => Err(RegistrationError::Forbidden),
            Self::NotFound => Err(RegistrationError::NotFound),
        }
    }
}

impl Actor {
    /// Whether the actor may manage the club owned by `owner_id`: the
    /// organizer, the club's owner and users assigned to the club.
    #[must_use]
    pub fn may_manage_club(&self, club_id: ClubId, owner_id: UserId) -> bool {
        self.manage_registrations() || owner_id == self.user_id || self.club_id == Some(club_id)
    }

    /// Decides access to a club; `None` is an unknown club.
    #[must_use]
    pub fn club_access(&self, club: Option<ClubFacts>) -> Access {
        match club {
            None => Access::NotFound,
            Some(club) if self.may_manage_club(club.id, club.owner_id) => Access::Allowed,
            Some(_) => Access::Forbidden,
        }
    }

    /// Decides whether the actor may change a rider, given the rider's club
    /// (`None`: unknown rider).
    #[must_use]
    pub fn may_change_rider(&self, rider_club: Option<ClubFacts>) -> Access {
        self.club_access(rider_club)
    }

    /// Decides whether the actor may change a judge, given the judge's club
    /// (`None`: unknown judge).
    #[must_use]
    pub fn may_change_judge(&self, judge_club: Option<ClubFacts>) -> Access {
        self.club_access(judge_club)
    }

    /// Decides whether the actor may rename or delete a group, given the
    /// club that created it: the organizer and users who may manage the
    /// owner club. `None` is a group without an owner club (only the
    /// organizer changes it).
    #[must_use]
    pub fn may_change_group(&self, owner_club: Option<ClubFacts>) -> Access {
        if self.manage_registrations() {
            Access::Allowed
        } else if owner_club.is_none() {
            Access::Forbidden
        } else {
            self.club_access(owner_club)
        }
    }

    /// Decides whether the actor may change an act (`None`: unknown act): the
    /// organizer and users who own or are assigned to the club of one of its
    /// riders or, for a group, the club that created it.
    #[must_use]
    pub fn may_change_act(&self, act: Option<&ActFacts>) -> Access {
        match act {
            None => Access::NotFound,
            Some(act)
                if self.manage_registrations()
                    || act.clubs.iter().any(|club| {
                        club.owner_id == self.user_id || Some(club.id) == self.club_id
                    }) =>
            {
                Access::Allowed
            }
            Some(_) => Access::Forbidden,
        }
    }
}

/// The former name of [`CommandError`], kept while the repo traits exist
/// (#238).
pub use crate::error::CommandError as RegistrationError;

/// A club with its owner and contact.
#[derive(Debug, Clone)]
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

/// A row of the club list.
#[derive(Debug, Clone)]
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

#[async_trait::async_trait]
pub trait ClubsRepo {
    /// Registers a club owned by the user, who is assigned to it.
    async fn create_club(&self, owner: UserId, name: &str) -> Result<ClubId, DbError>;

    /// Renames the club. With `only_owner`, only if that user owns it
    /// (nothing changes otherwise).
    async fn rename_club(
        &self,
        club: ClubId,
        name: &str,
        only_owner: Option<UserId>,
    ) -> Result<(), DbError>;

    /// Sets the club's Vereinsverantwortliche/r (both `None` clears it).
    async fn set_club_responsible(
        &self,
        actor: &Actor,
        club: ClubId,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Result<(), RegistrationError>;

    async fn find_club(&self, id: ClubId) -> Result<Option<ClubProfile>, DbError>;

    /// All clubs by name.
    async fn list_clubs(&self) -> Result<Vec<ClubRow>, DbError>;

    /// Fails with `NotFound` for an unknown club and `Forbidden` unless the
    /// actor may manage it: the organizer, the owner and users assigned to
    /// the club.
    async fn ensure_club_access(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<(), RegistrationError>;

    /// Fails unless the actor may change the act: the organizer and users
    /// with access to the club of one of its riders or, for groups, to the
    /// club that created it.
    async fn ensure_act_access(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError>;
}

/// Letters and digits that can't be confused with each other (no 0/O, 1/I).
const CLUB_CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A short code for a club's transfer reference, like "EV-K7Q3".
///
/// It comes from the random end of the club's id, so it stays the same when
/// the club is renamed and needs no storage. Four characters give about a
/// million codes; with a few dozen clubs a clash is very unlikely, and the
/// club name is part of the reference anyway.
///
/// ```
/// use db_core::clubs::club_code;
/// use db_core::ids::ClubId;
///
/// let id: ClubId = "0199d4a2-7c3e-7000-8000-00000000abcd".parse().unwrap();
/// assert_eq!(club_code(&id), club_code(&id));
/// assert!(club_code(&id).starts_with("EV-"));
/// assert_eq!(club_code(&id).len(), 7);
/// ```
pub fn club_code(id: &ClubId) -> String {
    let bytes = id.0.as_bytes();
    let bits = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let code: String = (0..4)
        .map(|i| CLUB_CODE_ALPHABET[((bits >> (5 * i)) & 31) as usize] as char)
        .collect();
    format!("EV-{code}")
}

#[cfg(test)]
mod access_tests {
    use super::*;

    fn id<T: std::str::FromStr>(n: u8) -> T
    where
        T::Err: std::fmt::Debug,
    {
        format!("0199d4a2-7c3e-7000-8000-0000000000{n:02x}")
            .parse()
            .unwrap()
    }

    fn actor(user: u8, club: Option<u8>, organizer: bool) -> Actor {
        Actor::new(
            id(user),
            "Max Müller",
            if organizer {
                vec![crate::roles::Role::Admin]
            } else {
                vec![]
            },
            club.map(id),
        )
    }

    fn club(club: u8, owner: u8) -> ClubFacts {
        ClubFacts {
            id: id(club),
            owner_id: id(owner),
        }
    }

    #[test]
    fn club_decisions() {
        let c = Some(club(1, 10));
        assert_eq!(actor(99, None, false).club_access(None), Access::NotFound);
        assert_eq!(actor(99, None, true).club_access(None), Access::NotFound);
        assert_eq!(actor(99, None, true).club_access(c), Access::Allowed);
        assert_eq!(actor(10, None, false).club_access(c), Access::Allowed);
        assert_eq!(actor(11, Some(1), false).club_access(c), Access::Allowed);
        assert_eq!(actor(11, Some(2), false).club_access(c), Access::Forbidden);
        assert_eq!(actor(11, None, false).club_access(c), Access::Forbidden);
    }

    #[test]
    fn rider_and_judge_follow_their_club() {
        let c = Some(club(1, 10));
        for a in [
            actor(99, None, true),
            actor(11, Some(1), false),
            actor(11, Some(2), false),
        ] {
            assert_eq!(a.may_change_rider(c), a.club_access(c));
            assert_eq!(a.may_change_judge(c), a.club_access(c));
            assert_eq!(a.may_change_rider(None), Access::NotFound);
            assert_eq!(a.may_change_judge(None), Access::NotFound);
        }
    }

    #[test]
    fn group_decisions() {
        let c = Some(club(1, 10));
        assert_eq!(
            actor(99, None, true).may_change_group(None),
            Access::Allowed
        );
        assert_eq!(actor(99, None, true).may_change_group(c), Access::Allowed);
        assert_eq!(actor(10, None, false).may_change_group(c), Access::Allowed);
        assert_eq!(
            actor(11, Some(1), false).may_change_group(c),
            Access::Allowed
        );
        assert_eq!(
            actor(11, Some(2), false).may_change_group(c),
            Access::Forbidden
        );
        assert_eq!(
            actor(10, Some(1), false).may_change_group(None),
            Access::Forbidden
        );
    }

    #[test]
    fn act_decisions() {
        // A pair of riders from clubs 1 (owner 10) and 2 (owner 20).
        let pair = ActFacts {
            clubs: vec![club(1, 10), club(2, 20)],
        };
        // A group created by club 3 (owner 30) with riders of club 1.
        let group = ActFacts {
            clubs: vec![club(3, 30), club(1, 10)],
        };
        let allowed = |a: Actor, f: &ActFacts| a.may_change_act(Some(f));
        assert_eq!(actor(99, None, true).may_change_act(None), Access::NotFound);
        assert_eq!(
            actor(1, Some(1), false).may_change_act(None),
            Access::NotFound
        );
        assert_eq!(allowed(actor(99, None, true), &pair), Access::Allowed);
        assert_eq!(allowed(actor(11, Some(2), false), &pair), Access::Allowed);
        assert_eq!(allowed(actor(10, None, false), &pair), Access::Allowed);
        assert_eq!(allowed(actor(11, Some(4), false), &pair), Access::Forbidden);
        assert_eq!(allowed(actor(30, None, false), &group), Access::Allowed);
        assert_eq!(allowed(actor(11, Some(3), false), &group), Access::Allowed);
        assert_eq!(allowed(actor(11, Some(1), false), &group), Access::Allowed);
        assert_eq!(
            allowed(actor(11, Some(2), false), &group),
            Access::Forbidden
        );
        let empty = ActFacts::default();
        assert_eq!(
            allowed(actor(11, Some(1), false), &empty),
            Access::Forbidden
        );
        assert_eq!(allowed(actor(99, None, true), &empty), Access::Allowed);
    }
}
