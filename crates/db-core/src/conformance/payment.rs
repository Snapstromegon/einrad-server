//! Payment and check-in through the trait: bookings, the payment log, fee
//! waivers and the check-in desk.

use time::macros::{date, datetime};

use super::{World, create_booking, set_fee_waived};
use crate::{
    Db,
    actor::Actor,
    bookings::{BookingAction, BookingInput},
    commands::{
        check_in::SetCheckedIn,
        club::AddRider,
        payment::{DeleteBooking, EditBooking},
    },
    error::CommandError,
    fee_waivers::{Target, WaiverLevel},
    gender::Gender,
    ids::{BookingId, ClubId, RiderId},
    queries::payment::ListBookingLog,
    roles::Role,
};

fn treasurer(w: &World) -> Actor {
    Actor::new(
        w.organizer.user_id,
        "Erika Musterfrau",
        vec![Role::Treasurer],
        None,
    )
}

fn input(amount: f64, club_id: Option<ClubId>) -> BookingInput {
    BookingInput {
        amount,
        date: date!(2027 - 03 - 01),
        note: "Startgeld".into(),
        club_id,
        ignored: false,
    }
}

async fn edit_booking(
    db: &dyn Db,
    actor: &Actor,
    booking_id: BookingId,
    input: BookingInput,
) -> Result<(), CommandError> {
    db.edit_booking(actor, EditBooking { booking_id, input })
        .await
}

async fn delete_booking(
    db: &dyn Db,
    actor: &Actor,
    booking_id: BookingId,
) -> Result<(), CommandError> {
    db.delete_booking(actor, DeleteBooking { booking_id }).await
}

async fn log_of(db: &dyn Db, actor: &Actor, club_id: Option<ClubId>) -> Vec<BookingAction> {
    db.list_booking_log(actor, ListBookingLog { club_id })
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.action)
        .collect()
}

/// Bookings are created, edited and deleted, each change writes one log
/// entry (none for an unchanged edit), the list is newest first, and the log
/// can be narrowed to a club.
pub async fn bookings_and_the_payment_log(db: &dyn Db) {
    let w = World::seed(db).await;
    let actor = treasurer(&w);
    assert!(db.list_bookings(&actor).await.unwrap().is_empty());

    // An unknown club is refused, and nothing is written.
    assert!(matches!(
        create_booking(db, &actor, input(5.0, Some(ClubId::new()))).await,
        Err(CommandError::NotFound)
    ));
    assert!(db.list_bookings(&actor).await.unwrap().is_empty());

    let first = create_booking(db, &actor, input(12.5, Some(w.club_a)))
        .await
        .unwrap();
    let second = create_booking(db, &actor, input(-3.0, None)).await.unwrap();
    let listed = db.list_bookings(&actor).await.unwrap();
    assert_eq!(listed.len(), 2);
    let a = listed.iter().find(|b| b.id == first).unwrap();
    assert_eq!(
        (a.amount, a.club_id, a.club_name.as_deref(), a.ignored),
        (12.5, Some(w.club_a), Some("RSV Adorf"), false)
    );

    // Assigning only changes the assignment; the unchanged edit logs nothing.
    edit_booking(db, &actor, second, input(-3.0, Some(w.club_b)))
        .await
        .unwrap();
    edit_booking(db, &actor, second, input(-3.0, Some(w.club_b)))
        .await
        .unwrap();
    edit_booking(db, &actor, first, input(20.0, Some(w.club_a)))
        .await
        .unwrap();
    assert!(matches!(
        edit_booking(db, &actor, BookingId::new(), input(1.0, None)).await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        edit_booking(db, &actor, first, input(1.0, Some(ClubId::new()))).await,
        Err(CommandError::NotFound)
    ));
    assert_eq!(
        log_of(db, &actor, None).await,
        [
            BookingAction::Edited,
            BookingAction::Assigned,
            BookingAction::Created,
            BookingAction::Created,
        ]
    );
    // The log of club B has the assignment only; club A's has both of its
    // booking's entries.
    assert_eq!(
        log_of(db, &actor, Some(w.club_b)).await,
        [BookingAction::Assigned]
    );
    assert_eq!(
        log_of(db, &actor, Some(w.club_a)).await,
        [BookingAction::Edited, BookingAction::Created]
    );
    let entry = db
        .list_booking_log(&actor, ListBookingLog { club_id: None })
        .await
        .unwrap()
        .remove(0);
    assert_eq!(entry.user_name.as_deref(), Some("Erika Musterfrau"));
    assert_eq!(entry.roles, [Role::Treasurer]);
    assert_eq!(entry.before.unwrap().amount, 12.5);
    assert_eq!(entry.after.unwrap().amount, 20.0);

    // Deleting removes the booking; the log keeps it.
    delete_booking(db, &actor, first).await.unwrap();
    assert!(matches!(
        delete_booking(db, &actor, first).await,
        Err(CommandError::NotFound)
    ));
    let listed = db.list_bookings(&actor).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, second);
    let log = log_of(db, &actor, None).await;
    assert_eq!(log[0], BookingAction::Deleted);
    assert_eq!(log.len(), 5);
}

/// Granting and revoking a fee waiver logs it with the clubs it concerns, a
/// waiver that changes nothing doesn't, and the log filters by club.
pub async fn fee_waivers_are_logged(db: &dyn Db) {
    let w = World::seed(db).await;
    let actor = treasurer(&w);
    let rider = db
        .add_rider(
            &w.owner_a,
            AddRider {
                club_id: w.club_a,
                firstname: "Anna".into(),
                lastname: "Muster".into(),
                birthdate: datetime!(2012-05-01 0:00 UTC),
                gender: Gender::Female,
            },
        )
        .await
        .unwrap();

    set_fee_waived(db, &actor, Target::Rider(rider), Some("Härtefall"))
        .await
        .unwrap();
    // The same reason again, and charging an unwaived club: nothing to log.
    set_fee_waived(db, &actor, Target::Rider(rider), Some("Härtefall"))
        .await
        .unwrap();
    set_fee_waived(db, &actor, Target::Club(w.club_b), None)
        .await
        .unwrap();
    assert!(matches!(
        set_fee_waived(db, &actor, Target::Club(ClubId::new()), Some("x")).await,
        Err(CommandError::NotFound)
    ));
    set_fee_waived(db, &actor, Target::Club(w.club_b), Some("Gast"))
        .await
        .unwrap();
    set_fee_waived(db, &actor, Target::Club(w.club_b), None)
        .await
        .unwrap();

    let log = db
        .list_booking_log(&actor, ListBookingLog { club_id: None })
        .await
        .unwrap();
    let actions: Vec<_> = log.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        [
            BookingAction::WaiverRevoked,
            BookingAction::Waived,
            BookingAction::Waived,
        ]
    );
    assert!(log.iter().all(|entry| entry.booking_id.is_none()));
    let revoked = log[0].waiver.as_ref().unwrap();
    assert_eq!(
        (revoked.level, revoked.reason.as_str()),
        (WaiverLevel::Club, "Gast")
    );
    let rider_waiver = log[2].waiver.as_ref().unwrap();
    assert_eq!(rider_waiver.level, WaiverLevel::Rider);
    assert_eq!(rider_waiver.name, "Anna Muster");
    assert_eq!(rider_waiver.clubs[0].id, w.club_a);
    // Narrowed to a club: the rider's waiver concerns club A only.
    assert_eq!(
        log_of(db, &actor, Some(w.club_a)).await,
        [BookingAction::Waived]
    );
    assert_eq!(log_of(db, &actor, Some(w.club_b)).await.len(), 2);
}

async fn add_rider(db: &dyn Db, w: &World, firstname: &str) -> RiderId {
    db.add_rider(
        &w.owner_a,
        AddRider {
            club_id: w.club_a,
            firstname: firstname.into(),
            lastname: "Muster".into(),
            birthdate: datetime!(2012-05-01 0:00 UTC),
            gender: Gender::Female,
        },
    )
    .await
    .unwrap()
}

async fn set_checked_in(
    db: &dyn Db,
    actor: &Actor,
    riders: &[RiderId],
    at: Option<time::OffsetDateTime>,
) -> Result<(), CommandError> {
    db.set_checked_in(
        actor,
        SetCheckedIn {
            rider_ids: riders.to_vec(),
            checked_in_at: at,
        },
    )
    .await
}

/// Check-in sets the time of riders that weren't in yet, keeps earlier
/// times, checks out with `None`, and fails as a whole for an unknown rider.
pub async fn check_in_sets_keeps_and_clears_times(db: &dyn Db) {
    let w = World::seed(db).await;
    let actor = treasurer(&w);
    let anna = add_rider(db, &w, "Anna").await;
    let berta = add_rider(db, &w, "Berta").await;
    let time_of = |all: &[crate::queries::check_in::CheckIn], id| {
        all.iter().find(|c| c.rider_id == id).unwrap().checked_in_at
    };

    let all = db.list_check_ins(&actor).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|c| c.checked_in_at.is_none()));

    let first = datetime!(2027-06-01 9:00 UTC);
    let later = datetime!(2027-06-01 10:00 UTC);
    set_checked_in(db, &actor, &[anna, anna], Some(first))
        .await
        .unwrap();
    set_checked_in(db, &actor, &[anna, berta], Some(later))
        .await
        .unwrap();
    let all = db.list_check_ins(&actor).await.unwrap();
    assert_eq!(time_of(&all, anna), Some(first));
    assert_eq!(time_of(&all, berta), Some(later));

    // An unknown rider fails the whole request; nobody changes.
    assert!(matches!(
        set_checked_in(db, &actor, &[anna, RiderId::new(), berta], None).await,
        Err(CommandError::NotFound)
    ));
    let all = db.list_check_ins(&actor).await.unwrap();
    assert_eq!(time_of(&all, anna), Some(first));
    assert_eq!(time_of(&all, berta), Some(later));

    set_checked_in(db, &actor, &[anna], None).await.unwrap();
    let all = db.list_check_ins(&actor).await.unwrap();
    assert_eq!(time_of(&all, anna), None);
    assert_eq!(time_of(&all, berta), Some(later));
}
