//! The rider registration rules, through the trait commands: names, singles,
//! pairs and replacing a rider. The German messages are the contract.

use time::macros::datetime;

use super::World;
use crate::{
    Db,
    clubs::{Actor, RegistrationError},
    commands::club::{
        AddRider, DeleteRider, DissolvePair, EditRider, LeavePair, LinkPair, ReplaceActRider,
        RequestPair, SetSingle,
    },
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
    queries::club::{ListClubPairRequests, ListClubRiders},
    riders::Pair,
};

async fn rider(db: &dyn Db, actor: &Actor, club: ClubId, name: &str) -> RiderId {
    let (firstname, lastname) = name.split_once(' ').unwrap();
    db.add_rider(
        actor,
        AddRider {
            club_id: club,
            firstname: firstname.into(),
            lastname: lastname.into(),
            birthdate: datetime!(2010-05-01 0:00 UTC),
            gender: Gender::Female,
        },
    )
    .await
    .unwrap()
}

/// The message of a rejected command.
fn rejection<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> String {
    match result.unwrap_err() {
        RegistrationError::Rejected(message) => message,
        other => panic!("not a message: {other:?}"),
    }
}

async fn pair_of(db: &dyn Db, rider: RiderId) -> Pair {
    db.rider_starts(rider).await.unwrap().pair
}

/// A club registers each name once, ignoring case and spaces; editing a rider
/// to their own name is fine.
pub async fn rider_names_are_unique_per_club(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna Muster").await;
    let new = |firstname: &str| AddRider {
        club_id: w.club_a,
        firstname: firstname.into(),
        lastname: " MUSTER ".into(),
        birthdate: datetime!(2010-05-01 0:00 UTC),
        gender: Gender::Female,
    };
    assert_eq!(
        rejection(db.add_rider(&w.owner_a, new(" anna")).await),
        "Anna Muster ist bereits gemeldet."
    );
    // Another club may have the name.
    rider(db, &w.owner_b, w.club_b, "Anna Muster").await;

    let berta = rider(db, &w.owner_a, w.club_a, "Berta Muster").await;
    let edit = |rider_id: RiderId, firstname: &str| EditRider {
        rider_id,
        firstname: firstname.into(),
        lastname: "Muster".into(),
        birthdate: datetime!(2010-05-01 0:00 UTC),
        gender: None,
    };
    assert_eq!(
        rejection(db.edit_rider(&w.owner_a, edit(berta, "ANNA")).await),
        "Anna Muster ist bereits gemeldet."
    );
    db.edit_rider(&w.owner_a, edit(anna, "Anna")).await.unwrap();
    db.edit_rider(&w.owner_a, edit(berta, "Clara"))
        .await
        .unwrap();
}

/// A single is added, kept when added again, and removed.
pub async fn single_on_and_off(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna Muster").await;
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);

    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: anna,
            single: true,
        },
    )
    .await
    .unwrap();
    let single = db.rider_starts(anna).await.unwrap().single;
    assert!(single.is_some());
    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: anna,
            single: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(db.rider_starts(anna).await.unwrap().single, single);

    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: anna,
            single: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);
    db.set_single(
        &w.owner_a,
        SetSingle {
            rider_id: anna,
            single: false,
        },
    )
    .await
    .unwrap();
}

/// Pair requests: riders naming each other (or a waiting rider naming nobody)
/// pair at once; others wait; nobody asks for themselves or from a pair.
pub async fn pair_requests_match_waiting_riders(db: &dyn Db) {
    let w = World::seed(db).await;
    let a = &w.owner_a;
    let anna = rider(db, a, w.club_a, "Anna Muster").await;
    let berta = rider(db, a, w.club_a, "Berta Muster").await;
    let clara = rider(db, a, w.club_a, "Clara Muster").await;
    let dora = rider(db, a, w.club_a, "Dora Muster").await;
    let zoe = rider(db, &w.owner_b, w.club_b, "Zoe Muster").await;

    assert_eq!(
        rejection(
            db.request_pair(
                a,
                RequestPair {
                    rider_id: anna,
                    partner_name: Some(" Anna Muster ".into())
                }
            )
            .await
        ),
        "Fahrer und Partner sind die selbe Person!"
    );
    // A blank name is no name.
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: anna,
                partner_name: Some("  ".into())
            }
        )
        .await
        .unwrap(),
        None
    );
    assert_eq!(
        pair_of(db, anna).await,
        Pair::Requested { partner_name: None }
    );
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: anna,
                partner_name: Some("Clara Muster".into())
            }
        )
        .await
        .unwrap(),
        None
    );
    assert_eq!(
        pair_of(db, anna).await,
        Pair::Requested {
            partner_name: Some("Clara Muster".into())
        }
    );

    // Anna waits for Clara: Berta naming Anna isn't taken.
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: berta,
                partner_name: Some("Anna Muster".into())
            }
        )
        .await
        .unwrap(),
        None
    );
    // Another club's rider is never taken.
    db.request_pair(
        &w.owner_b,
        RequestPair {
            rider_id: zoe,
            partner_name: Some("Dora Muster".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: dora,
                partner_name: Some("Zoe Muster".into())
            }
        )
        .await
        .unwrap(),
        None
    );
    // Clara names Anna back: they pair.
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: clara,
                partner_name: Some("Anna Muster".into())
            }
        )
        .await
        .unwrap(),
        Some(anna)
    );
    let Pair::Linked { act_id, partner } = pair_of(db, anna).await else {
        panic!("not linked");
    };
    assert_eq!(partner, Some(clara));
    assert_eq!(
        pair_of(db, clara).await,
        Pair::Linked {
            act_id,
            partner: Some(anna)
        }
    );
    assert_eq!(
        rejection(
            db.request_pair(
                a,
                RequestPair {
                    rider_id: anna,
                    partner_name: None
                }
            )
            .await
        ),
        "Anna Muster fährt schon eine Paarkür."
    );
    // Berta still waits for the paired Anna: she names Anna, not Dora.
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: dora,
                partner_name: Some("Berta Muster".into())
            }
        )
        .await
        .unwrap(),
        None
    );
    // A waiting rider who names nobody is taken by whoever names them.
    db.request_pair(
        a,
        RequestPair {
            rider_id: berta,
            partner_name: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        db.request_pair(
            a,
            RequestPair {
                rider_id: dora,
                partner_name: Some("Berta Muster".into())
            }
        )
        .await
        .unwrap(),
        Some(berta)
    );
}

/// Linking: clubs link their own riders, the organizer riders of any club;
/// one pair per rider.
pub async fn link_pair_rules(db: &dyn Db) {
    let w = World::seed(db).await;
    let a = &w.owner_a;
    let anna = rider(db, a, w.club_a, "Anna Muster").await;
    let berta = rider(db, a, w.club_a, "Berta Muster").await;
    let clara = rider(db, a, w.club_a, "Clara Muster").await;
    let zoe = rider(db, &w.owner_b, w.club_b, "Zoe Muster").await;

    assert_eq!(
        rejection(
            db.link_pair(
                a,
                LinkPair {
                    rider_id: anna,
                    partner_id: anna
                }
            )
            .await
        ),
        "Fahrer und Partner sind die selbe Person!"
    );
    assert!(
        rejection(
            db.link_pair(
                a,
                LinkPair {
                    rider_id: anna,
                    partner_id: zoe
                }
            )
            .await
        )
        .contains("muss aus dem eigenen Verein sein")
    );
    db.request_pair(
        a,
        RequestPair {
            rider_id: berta,
            partner_name: Some("Nobody Known".into()),
        },
    )
    .await
    .unwrap();
    let act = db
        .link_pair(
            a,
            LinkPair {
                rider_id: anna,
                partner_id: berta,
            },
        )
        .await
        .unwrap();
    // The request of the partner is fulfilled.
    assert_eq!(
        pair_of(db, berta).await,
        Pair::Linked {
            act_id: act,
            partner: Some(anna)
        }
    );
    assert_eq!(
        rejection(
            db.link_pair(
                a,
                LinkPair {
                    rider_id: clara,
                    partner_id: anna
                }
            )
            .await
        ),
        "Anna Muster fährt schon eine Paarkür."
    );
    assert_eq!(
        rejection(
            db.link_pair(
                a,
                LinkPair {
                    rider_id: anna,
                    partner_id: clara
                }
            )
            .await
        ),
        "Anna Muster fährt schon eine Paarkür."
    );
    assert_eq!(
        rejection(
            db.link_pair(
                a,
                LinkPair {
                    rider_id: clara,
                    partner_id: berta
                }
            )
            .await
        ),
        "Berta Muster fährt schon eine Paarkür."
    );
    // The organizer links across clubs.
    let cross = db
        .link_pair(
            &w.organizer,
            LinkPair {
                rider_id: clara,
                partner_id: zoe,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        pair_of(db, zoe).await,
        Pair::Linked {
            act_id: cross,
            partner: Some(clara)
        }
    );
}

/// Leaving a pair leaves the partner waiting; dissolving leaves nobody
/// waiting and only works on pair acts.
pub async fn leave_and_dissolve_pairs(db: &dyn Db) {
    let w = World::seed(db).await;
    let a = &w.owner_a;
    let anna = rider(db, a, w.club_a, "Anna Muster").await;
    let berta = rider(db, a, w.club_a, "Berta Muster").await;
    let clara = rider(db, a, w.club_a, "Clara Muster").await;
    let dora = rider(db, a, w.club_a, "Dora Muster").await;

    db.link_pair(
        a,
        LinkPair {
            rider_id: anna,
            partner_id: berta,
        },
    )
    .await
    .unwrap();
    db.leave_pair(a, LeavePair { rider_id: anna })
        .await
        .unwrap();
    assert_eq!(pair_of(db, anna).await, Pair::None);
    assert_eq!(
        pair_of(db, berta).await,
        Pair::Requested { partner_name: None }
    );
    // A waiting rider just stops waiting; leaving without a pair is fine.
    db.leave_pair(a, LeavePair { rider_id: berta })
        .await
        .unwrap();
    assert_eq!(pair_of(db, berta).await, Pair::None);
    db.leave_pair(a, LeavePair { rider_id: berta })
        .await
        .unwrap();

    let pair = db
        .link_pair(
            a,
            LinkPair {
                rider_id: clara,
                partner_id: dora,
            },
        )
        .await
        .unwrap();
    db.set_single(
        a,
        SetSingle {
            rider_id: clara,
            single: true,
        },
    )
    .await
    .unwrap();
    let single: ActId = db.rider_starts(clara).await.unwrap().single.unwrap();
    assert_eq!(
        rejection(db.dissolve_pair(a, DissolvePair { act_id: single }).await),
        "Die Kür ist keine Paarkür."
    );
    assert!(matches!(
        db.dissolve_pair(
            &w.organizer,
            DissolvePair {
                act_id: ActId::new()
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));
    db.dissolve_pair(a, DissolvePair { act_id: pair })
        .await
        .unwrap();
    assert_eq!(pair_of(db, clara).await, Pair::None);
    assert_eq!(pair_of(db, dora).await, Pair::None);

    // Deleting a rider of a pair leaves the partner waiting.
    db.link_pair(
        a,
        LinkPair {
            rider_id: clara,
            partner_id: dora,
        },
    )
    .await
    .unwrap();
    db.delete_rider(a, DeleteRider { rider_id: clara })
        .await
        .unwrap();
    assert_eq!(
        pair_of(db, dora).await,
        Pair::Requested { partner_name: None }
    );
}

/// Replacing a rider of a single or pair act.
pub async fn replace_act_rider_rules(db: &dyn Db) {
    let w = World::seed(db).await;
    let a = &w.owner_a;
    let anna = rider(db, a, w.club_a, "Anna Muster").await;
    let berta = rider(db, a, w.club_a, "Berta Muster").await;
    let clara = rider(db, a, w.club_a, "Clara Muster").await;
    let zoe = rider(db, &w.owner_b, w.club_b, "Zoe Muster").await;

    db.set_single(
        a,
        SetSingle {
            rider_id: anna,
            single: true,
        },
    )
    .await
    .unwrap();
    db.set_single(
        a,
        SetSingle {
            rider_id: berta,
            single: true,
        },
    )
    .await
    .unwrap();
    let single = db.rider_starts(anna).await.unwrap().single.unwrap();
    assert_eq!(
        rejection(
            db.replace_act_rider(
                a,
                ReplaceActRider {
                    act_id: single,
                    rider_id: anna,
                    new_rider_id: berta
                }
            )
            .await
        ),
        "Berta Muster fährt schon eine Einzelkür."
    );
    assert!(matches!(
        db.replace_act_rider(
            a,
            ReplaceActRider {
                act_id: single,
                rider_id: berta,
                new_rider_id: clara
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));
    assert!(matches!(
        db.replace_act_rider(
            a,
            ReplaceActRider {
                act_id: ActId::new(),
                rider_id: anna,
                new_rider_id: clara
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));
    assert_eq!(
        rejection(
            db.replace_act_rider(
                &w.organizer,
                ReplaceActRider {
                    act_id: single,
                    rider_id: anna,
                    new_rider_id: anna
                }
            )
            .await
        ),
        "Der Fahrer fährt diese Kür schon."
    );
    db.replace_act_rider(
        a,
        ReplaceActRider {
            act_id: single,
            rider_id: anna,
            new_rider_id: clara,
        },
    )
    .await
    .unwrap();
    assert_eq!(db.rider_starts(clara).await.unwrap().single, Some(single));
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);
    // The owner of club A may not reach into club B; the organizer may.
    assert!(matches!(
        db.replace_act_rider(
            a,
            ReplaceActRider {
                act_id: single,
                rider_id: clara,
                new_rider_id: zoe
            }
        )
        .await,
        Err(RegistrationError::Forbidden)
    ));
    db.replace_act_rider(
        &w.organizer,
        ReplaceActRider {
            act_id: single,
            rider_id: clara,
            new_rider_id: zoe,
        },
    )
    .await
    .unwrap();
    assert_eq!(db.rider_starts(zoe).await.unwrap().single, Some(single));

    // Pairs: the new rider's request is fulfilled; riders of a pair can't be
    // taken; the partner already rides the act.
    let dora = rider(db, a, w.club_a, "Dora Muster").await;
    let erna = rider(db, a, w.club_a, "Erna Muster").await;
    let pair = db
        .link_pair(
            a,
            LinkPair {
                rider_id: anna,
                partner_id: berta,
            },
        )
        .await
        .unwrap();
    db.request_pair(
        a,
        RequestPair {
            rider_id: clara,
            partner_name: Some("Nobody Known".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        rejection(
            db.replace_act_rider(
                a,
                ReplaceActRider {
                    act_id: pair,
                    rider_id: anna,
                    new_rider_id: berta
                }
            )
            .await
        ),
        "Der Fahrer fährt diese Kür schon."
    );
    db.link_pair(
        a,
        LinkPair {
            rider_id: dora,
            partner_id: erna,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        rejection(
            db.replace_act_rider(
                a,
                ReplaceActRider {
                    act_id: pair,
                    rider_id: anna,
                    new_rider_id: dora
                }
            )
            .await
        ),
        "Dora Muster fährt schon eine Paarkür."
    );
    db.replace_act_rider(
        a,
        ReplaceActRider {
            act_id: pair,
            rider_id: anna,
            new_rider_id: clara,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        pair_of(db, clara).await,
        Pair::Linked {
            act_id: pair,
            partner: Some(berta)
        }
    );
    assert_eq!(pair_of(db, anna).await, Pair::None);
}

/// The rider and pair-request lists: all of them for who may see all, the
/// club's own for the club; a stranger's club is off limits.
pub async fn rider_and_pair_request_lists(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna Muster").await;
    let zoe = rider(db, &w.owner_b, w.club_b, "Zoe Muster").await;
    db.request_pair(
        &w.owner_a,
        RequestPair {
            rider_id: anna,
            partner_name: None,
        },
    )
    .await
    .unwrap();
    db.set_single(
        &w.owner_b,
        SetSingle {
            rider_id: zoe,
            single: true,
        },
    )
    .await
    .unwrap();

    let all = db.list_riders(&w.organizer).await.unwrap();
    assert_eq!(all.len(), 2);
    let zoe_row = all.iter().find(|r| r.id == zoe).unwrap();
    assert_eq!(zoe_row.club_name, "RC Beistadt");
    assert_eq!(zoe_row.club_id, w.club_b);

    let own = db
        .list_club_riders(&w.owner_a, ListClubRiders { club_id: w.club_a })
        .await
        .unwrap();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].id, anna);
    assert_eq!(
        db.list_club_riders(&w.organizer, ListClubRiders { club_id: w.club_b })
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        db.list_club_riders(&w.owner_a, ListClubRiders { club_id: w.club_b })
            .await,
        Err(RegistrationError::Forbidden)
    ));

    let requests = db.list_pair_requests(&w.organizer).await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].rider_id, anna);
    assert_eq!(requests[0].partner_name, None);
    let own = db
        .list_club_pair_requests(&w.owner_a, ListClubPairRequests { club_id: w.club_a })
        .await
        .unwrap();
    assert_eq!(own.len(), 1);
    assert!(
        db.list_club_pair_requests(&w.owner_b, ListClubPairRequests { club_id: w.club_b })
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        db.list_club_pair_requests(&w.owner_b, ListClubPairRequests { club_id: w.club_a })
            .await,
        Err(RegistrationError::Forbidden)
    ));
}
