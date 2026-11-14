//! The rider registration rules, through the trait commands: names, singles,
//! pairs and replacing a rider. The German messages are the contract.

use time::macros::datetime;

use super::World;
use crate::{
    Db,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
    riders::{NewRider, Pair, RiderEdit},
};

async fn rider(db: &dyn Db, actor: &Actor, club: ClubId, name: &str) -> RiderId {
    let (firstname, lastname) = name.split_once(' ').unwrap();
    db.add_rider(
        actor,
        &NewRider {
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
    let new = |firstname: &str| NewRider {
        club_id: w.club_a,
        firstname: firstname.into(),
        lastname: " MUSTER ".into(),
        birthdate: datetime!(2010-05-01 0:00 UTC),
        gender: Gender::Female,
    };
    assert_eq!(
        rejection(db.add_rider(&w.owner_a, &new(" anna")).await),
        "Anna Muster ist bereits gemeldet."
    );
    // Another club may have the name.
    rider(db, &w.owner_b, w.club_b, "Anna Muster").await;

    let berta = rider(db, &w.owner_a, w.club_a, "Berta Muster").await;
    let edit = |firstname: &str| RiderEdit {
        firstname: firstname.into(),
        lastname: "Muster".into(),
        birthdate: datetime!(2010-05-01 0:00 UTC),
        gender: None,
    };
    assert_eq!(
        rejection(db.edit_rider(&w.owner_a, berta, &edit("ANNA")).await),
        "Anna Muster ist bereits gemeldet."
    );
    db.edit_rider(&w.owner_a, anna, &edit("Anna"))
        .await
        .unwrap();
    db.edit_rider(&w.owner_a, berta, &edit("Clara"))
        .await
        .unwrap();
}

/// A single is added, kept when added again, and removed.
pub async fn single_on_and_off(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna Muster").await;
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);

    db.set_single(&w.owner_a, anna, true).await.unwrap();
    let single = db.rider_starts(anna).await.unwrap().single;
    assert!(single.is_some());
    db.set_single(&w.owner_a, anna, true).await.unwrap();
    assert_eq!(db.rider_starts(anna).await.unwrap().single, single);

    db.set_single(&w.owner_a, anna, false).await.unwrap();
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);
    db.set_single(&w.owner_a, anna, false).await.unwrap();
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
        rejection(db.request_pair(a, anna, Some(" Anna Muster ")).await),
        "Fahrer und Partner sind die selbe Person!"
    );
    // A blank name is no name.
    assert_eq!(db.request_pair(a, anna, Some("  ")).await.unwrap(), None);
    assert_eq!(
        pair_of(db, anna).await,
        Pair::Requested { partner_name: None }
    );
    assert_eq!(
        db.request_pair(a, anna, Some("Clara Muster"))
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
        db.request_pair(a, berta, Some("Anna Muster"))
            .await
            .unwrap(),
        None
    );
    // Another club's rider is never taken.
    db.request_pair(&w.owner_b, zoe, Some("Dora Muster"))
        .await
        .unwrap();
    assert_eq!(
        db.request_pair(a, dora, Some("Zoe Muster")).await.unwrap(),
        None
    );
    // Clara names Anna back: they pair.
    assert_eq!(
        db.request_pair(a, clara, Some("Anna Muster"))
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
        rejection(db.request_pair(a, anna, None).await),
        "Anna Muster fährt schon eine Paarkür."
    );
    // Berta still waits for the paired Anna: she names Anna, not Dora.
    assert_eq!(
        db.request_pair(a, dora, Some("Berta Muster"))
            .await
            .unwrap(),
        None
    );
    // A waiting rider who names nobody is taken by whoever names them.
    db.request_pair(a, berta, None).await.unwrap();
    assert_eq!(
        db.request_pair(a, dora, Some("Berta Muster"))
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
        rejection(db.link_pair(a, anna, anna).await),
        "Fahrer und Partner sind die selbe Person!"
    );
    assert!(
        rejection(db.link_pair(a, anna, zoe).await).contains("muss aus dem eigenen Verein sein")
    );
    db.request_pair(a, berta, Some("Nobody Known"))
        .await
        .unwrap();
    let act = db.link_pair(a, anna, berta).await.unwrap();
    // The request of the partner is fulfilled.
    assert_eq!(
        pair_of(db, berta).await,
        Pair::Linked {
            act_id: act,
            partner: Some(anna)
        }
    );
    assert_eq!(
        rejection(db.link_pair(a, clara, anna).await),
        "Anna Muster fährt schon eine Paarkür."
    );
    assert_eq!(
        rejection(db.link_pair(a, anna, clara).await),
        "Anna Muster fährt schon eine Paarkür."
    );
    assert_eq!(
        rejection(db.link_pair(a, clara, berta).await),
        "Berta Muster fährt schon eine Paarkür."
    );
    // The organizer links across clubs.
    let cross = db.link_pair(&w.organizer, clara, zoe).await.unwrap();
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

    db.link_pair(a, anna, berta).await.unwrap();
    db.leave_pair(a, anna).await.unwrap();
    assert_eq!(pair_of(db, anna).await, Pair::None);
    assert_eq!(
        pair_of(db, berta).await,
        Pair::Requested { partner_name: None }
    );
    // A waiting rider just stops waiting; leaving without a pair is fine.
    db.leave_pair(a, berta).await.unwrap();
    assert_eq!(pair_of(db, berta).await, Pair::None);
    db.leave_pair(a, berta).await.unwrap();

    let pair = db.link_pair(a, clara, dora).await.unwrap();
    db.set_single(a, clara, true).await.unwrap();
    let single: ActId = db.rider_starts(clara).await.unwrap().single.unwrap();
    assert_eq!(
        rejection(db.dissolve_pair(a, single).await),
        "Die Kür ist keine Paarkür."
    );
    assert!(matches!(
        db.dissolve_pair(&w.organizer, ActId::new()).await,
        Err(RegistrationError::NotFound)
    ));
    db.dissolve_pair(a, pair).await.unwrap();
    assert_eq!(pair_of(db, clara).await, Pair::None);
    assert_eq!(pair_of(db, dora).await, Pair::None);

    // Deleting a rider of a pair leaves the partner waiting.
    db.link_pair(a, clara, dora).await.unwrap();
    db.delete_rider(a, clara).await.unwrap();
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

    db.set_single(a, anna, true).await.unwrap();
    db.set_single(a, berta, true).await.unwrap();
    let single = db.rider_starts(anna).await.unwrap().single.unwrap();
    assert_eq!(
        rejection(db.replace_act_rider(a, single, anna, berta).await),
        "Berta Muster fährt schon eine Einzelkür."
    );
    assert!(matches!(
        db.replace_act_rider(a, single, berta, clara).await,
        Err(RegistrationError::NotFound)
    ));
    assert!(matches!(
        db.replace_act_rider(a, ActId::new(), anna, clara).await,
        Err(RegistrationError::NotFound)
    ));
    assert_eq!(
        rejection(db.replace_act_rider(&w.organizer, single, anna, anna).await),
        "Der Fahrer fährt diese Kür schon."
    );
    db.replace_act_rider(a, single, anna, clara).await.unwrap();
    assert_eq!(db.rider_starts(clara).await.unwrap().single, Some(single));
    assert_eq!(db.rider_starts(anna).await.unwrap().single, None);
    // The owner of club A may not reach into club B; the organizer may.
    assert!(matches!(
        db.replace_act_rider(a, single, clara, zoe).await,
        Err(RegistrationError::Forbidden)
    ));
    db.replace_act_rider(&w.organizer, single, clara, zoe)
        .await
        .unwrap();
    assert_eq!(db.rider_starts(zoe).await.unwrap().single, Some(single));

    // Pairs: the new rider's request is fulfilled; riders of a pair can't be
    // taken; the partner already rides the act.
    let dora = rider(db, a, w.club_a, "Dora Muster").await;
    let erna = rider(db, a, w.club_a, "Erna Muster").await;
    let pair = db.link_pair(a, anna, berta).await.unwrap();
    db.request_pair(a, clara, Some("Nobody Known"))
        .await
        .unwrap();
    assert_eq!(
        rejection(db.replace_act_rider(a, pair, anna, berta).await),
        "Der Fahrer fährt diese Kür schon."
    );
    db.link_pair(a, dora, erna).await.unwrap();
    assert_eq!(
        rejection(db.replace_act_rider(a, pair, anna, dora).await),
        "Dora Muster fährt schon eine Paarkür."
    );
    db.replace_act_rider(a, pair, anna, clara).await.unwrap();
    assert_eq!(
        pair_of(db, clara).await,
        Pair::Linked {
            act_id: pair,
            partner: Some(berta)
        }
    );
    assert_eq!(pair_of(db, anna).await, Pair::None);
}
