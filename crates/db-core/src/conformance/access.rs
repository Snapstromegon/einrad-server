//! Who may change what: 403 versus 404 versus allowed, through the trait
//! methods that enforce it.

use time::macros::datetime;

use super::World;
use crate::{
    Db,
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, JudgeId, RiderId},
    jury::JudgeFields,
    riders::{NewRider, RiderEdit},
};

fn is_forbidden<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> bool {
    matches!(result, Err(RegistrationError::Forbidden))
}

fn is_not_found<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> bool {
    matches!(result, Err(RegistrationError::NotFound))
}

async fn add_rider(db: &dyn Db, actor: &Actor, club: ClubId, firstname: &str) -> RiderId {
    db.add_rider(
        actor,
        &NewRider {
            club_id: club,
            firstname: firstname.into(),
            lastname: "Testfahrer".into(),
            birthdate: datetime!(2010-05-01 0:00 UTC),
            gender: Gender::Female,
        },
    )
    .await
    .unwrap()
}

fn rider_edit(firstname: &str) -> RiderEdit {
    RiderEdit {
        firstname: firstname.into(),
        lastname: "Testfahrer".into(),
        birthdate: datetime!(2010-05-01 0:00 UTC),
        gender: None,
    }
}

/// Clubs: the organizer, the owner and assigned users manage a club; other
/// users don't; an unknown club is 404 even for the organizer.
pub async fn club_access_decisions(db: &dyn Db) {
    let w = World::seed(db).await;
    for allowed in [&w.organizer, &w.owner_a, &w.member_a] {
        db.ensure_club_access(allowed, w.club_a).await.unwrap();
    }
    for denied in [&w.owner_b, &w.stranger] {
        assert!(is_forbidden(db.ensure_club_access(denied, w.club_a).await));
    }
    for actor in [&w.organizer, &w.owner_a, &w.stranger] {
        assert!(is_not_found(
            db.ensure_club_access(actor, ClubId::new()).await
        ));
    }
    // Registering a rider needs access to the rider's club.
    assert!(is_forbidden(
        db.add_rider(
            &w.owner_b,
            &NewRider {
                club_id: w.club_a,
                firstname: "Anna".into(),
                lastname: "Testfahrer".into(),
                birthdate: datetime!(2010-05-01 0:00 UTC),
                gender: Gender::Female,
            },
        )
        .await
    ));
    assert!(is_not_found(
        db.add_rider(
            &w.organizer,
            &NewRider {
                club_id: ClubId::new(),
                firstname: "Anna".into(),
                lastname: "Testfahrer".into(),
                birthdate: datetime!(2010-05-01 0:00 UTC),
                gender: Gender::Female,
            },
        )
        .await
    ));
}

/// Riders follow their club.
pub async fn rider_access_decisions(db: &dyn Db) {
    let w = World::seed(db).await;
    let rider = add_rider(db, &w.owner_a, w.club_a, "Anna").await;
    assert!(is_forbidden(
        db.edit_rider(&w.owner_b, rider, &rider_edit("Berta")).await
    ));
    assert!(is_forbidden(
        db.edit_rider(&w.stranger, rider, &rider_edit("Berta"))
            .await
    ));
    assert!(is_forbidden(db.delete_rider(&w.owner_b, rider).await));
    assert!(is_not_found(
        db.edit_rider(&w.organizer, RiderId::new(), &rider_edit("Berta"))
            .await
    ));
    assert!(is_not_found(
        db.delete_rider(&w.owner_a, RiderId::new()).await
    ));
    db.edit_rider(&w.member_a, rider, &rider_edit("Berta"))
        .await
        .unwrap();
    db.edit_rider(&w.organizer, rider, &rider_edit("Carla"))
        .await
        .unwrap();
    db.delete_rider(&w.owner_a, rider).await.unwrap();
}

fn judge_fields() -> JudgeFields {
    JudgeFields {
        firstname: "Paula".into(),
        lastname: "Prüfer".into(),
        birthdate: datetime!(1990-01-01 0:00 UTC),
        is_trained: true,
        max_deployments: None,
        note: None,
        registrations: vec![],
    }
}

/// Judges follow their club.
pub async fn judge_access_decisions(db: &dyn Db) {
    let w = World::seed(db).await;
    assert!(is_forbidden(
        db.add_judge(&w.owner_b, w.club_a, judge_fields()).await
    ));
    assert!(is_not_found(
        db.add_judge(&w.organizer, ClubId::new(), judge_fields())
            .await
    ));
    let judge = db
        .add_judge(&w.member_a, w.club_a, judge_fields())
        .await
        .unwrap();
    assert!(is_forbidden(
        db.edit_judge(&w.owner_b, judge, judge_fields()).await
    ));
    assert!(is_forbidden(db.delete_judge(&w.stranger, judge).await));
    assert!(is_not_found(
        db.delete_judge(&w.organizer, JudgeId::new()).await
    ));
    db.edit_judge(&w.owner_a, judge, judge_fields())
        .await
        .unwrap();
    db.delete_judge(&w.organizer, judge).await.unwrap();
}

/// A pair act is changed by users of either rider's club, and the organizer.
pub async fn act_access_decisions(db: &dyn Db) {
    let w = World::seed(db).await;
    let a = add_rider(db, &w.owner_a, w.club_a, "Anna").await;
    let b = add_rider(db, &w.owner_b, w.club_b, "Berta").await;
    let act = db.link_pair(&w.organizer, a, b).await.unwrap();
    for allowed in [&w.organizer, &w.owner_a, &w.member_a, &w.owner_b] {
        db.ensure_act_access(allowed, act).await.unwrap();
    }
    assert!(is_forbidden(db.ensure_act_access(&w.stranger, act).await));
    for actor in [&w.organizer, &w.owner_a, &w.stranger] {
        assert!(is_not_found(
            db.ensure_act_access(actor, ActId::new()).await
        ));
    }
}

/// A group is changed by the club that created it and by the clubs of its
/// riders; renaming and deleting belong to the owner club only.
pub async fn group_access_decisions(db: &dyn Db) {
    let w = World::seed(db).await;
    let group = db
        .create_group(&w.owner_a, w.club_a, ActType::GroupSmall, "Wirbelwind")
        .await
        .unwrap();
    for allowed in [&w.organizer, &w.owner_a, &w.member_a] {
        db.ensure_act_access(allowed, group).await.unwrap();
    }
    for denied in [&w.owner_b, &w.stranger] {
        assert!(is_forbidden(db.ensure_act_access(denied, group).await));
        assert!(is_forbidden(db.rename_group(denied, group, "Neu").await));
        assert!(is_forbidden(db.delete_group(denied, group).await));
    }
    assert!(is_not_found(
        db.delete_group(&w.organizer, ActId::new()).await
    ));
    assert!(is_not_found(
        db.rename_group(&w.owner_a, ActId::new(), "Neu").await
    ));
    db.rename_group(&w.member_a, group, "Sturmwind")
        .await
        .unwrap();
    db.delete_group(&w.owner_a, group).await.unwrap();
}
