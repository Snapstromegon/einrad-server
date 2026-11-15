//! Clubs and groups through the trait: the club commands and queries, the
//! group commands and queries. Access decisions are in `access.rs`, the group
//! rules in `fees.rs`.

use time::macros::{date, datetime};

use super::{World, create_group, delete_group, rename_group, set_members, timeplan};
use crate::{
    Db,
    act_type::ActType,
    bookings,
    clubs::{Actor, RegistrationError},
    commands::{
        admin::SetActWithdrawn,
        club::{AddRider, CreateClub, EditClubAct, RenameClub, SetClubResponsible, SetSingle},
        group::{GroupMember, SetClubGroupType},
    },
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
    queries::{
        club::{ClubActs, ClubProfile, GetClub, ListClubActs},
        group::ListClubGroups,
    },
};

async fn rider(db: &dyn Db, actor: &Actor, club: ClubId, firstname: &str) -> RiderId {
    db.add_rider(
        actor,
        AddRider {
            club_id: club,
            firstname: firstname.into(),
            lastname: "Muster".into(),
            birthdate: datetime!(2012-05-01 0:00 UTC),
            gender: Gender::Female,
        },
    )
    .await
    .unwrap()
}

/// A single act of the rider.
async fn single_act(db: &dyn Db, actor: &Actor, club: ClubId, firstname: &str) -> ActId {
    let rider = rider(db, actor, club, firstname).await;
    db.set_single(
        actor,
        SetSingle {
            rider_id: rider,
            single: true,
        },
    )
    .await
    .unwrap();
    db.list_acts()
        .await
        .unwrap()
        .into_iter()
        .find(|act| act.participants.iter().any(|p| p.firstname == firstname))
        .unwrap()
        .id
}

async fn rename(
    db: &dyn Db,
    actor: &Actor,
    club_id: ClubId,
    name: &str,
) -> Result<(), RegistrationError> {
    db.rename_club(
        actor,
        RenameClub {
            club_id,
            name: name.into(),
        },
    )
    .await
}

async fn name_of(db: &dyn Db, club_id: ClubId) -> String {
    db.get_club(None, GetClub { club_id })
        .await
        .unwrap()
        .profile
        .name
}

async fn responsible(
    db: &dyn Db,
    actor: &Actor,
    club_id: ClubId,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<(), RegistrationError> {
    db.set_club_responsible(
        actor,
        SetClubResponsible {
            club_id,
            name: name.map(str::to_string),
            email: email.map(str::to_string),
        },
    )
    .await
}

async fn profile(db: &dyn Db, actor: &Actor, club_id: ClubId) -> ClubProfile {
    db.get_club(Some(actor), GetClub { club_id })
        .await
        .unwrap()
        .profile
}

async fn list_acts_of(
    db: &dyn Db,
    actor: &Actor,
    club_id: ClubId,
) -> Result<ClubActs, RegistrationError> {
    db.list_club_acts(actor, ListClubActs { club_id }).await
}

async fn edit(
    db: &dyn Db,
    actor: &Actor,
    act_id: ActId,
    name: &str,
    description: Option<&str>,
) -> Result<(), RegistrationError> {
    db.edit_club_act(
        actor,
        EditClubAct {
            act_id,
            name: name.into(),
            description: description.map(str::to_string),
        },
    )
    .await
}

async fn withdraw(
    db: &dyn Db,
    actor: &Actor,
    act_id: ActId,
    withdrawn: bool,
) -> Result<(), RegistrationError> {
    db.set_act_withdrawn(actor, SetActWithdrawn { act_id, withdrawn })
        .await
}

/// A club is created by its owner, who is assigned to it; the organizer
/// renames every club, an owner only their own (anyone else changes nothing,
/// without an error). The responsible person falls back to the owner.
pub async fn club_create_rename_and_responsible(db: &dyn Db) {
    let w = World::seed(db).await;
    let user = World::user(db, "Lea Neu").await;
    let newcomer = Actor::new(user, "Lea Neu", Vec::new(), None);
    let club = db
        .create_club(
            &newcomer,
            CreateClub {
                name: "RV Neustadt".into(),
            },
        )
        .await
        .unwrap();
    let view = db
        .get_club(Some(&newcomer), GetClub { club_id: club })
        .await
        .unwrap();
    assert_eq!(view.profile.name, "RV Neustadt");
    assert_eq!(view.profile.owner_id, user);

    // Not the owner: nothing changes, no error.
    rename(db, &w.stranger, club, "Kapern").await.unwrap();
    rename(db, &w.owner_a, club, "Kapern").await.unwrap();
    assert_eq!(name_of(db, club).await, "RV Neustadt");
    rename(db, &newcomer, club, "RV Neustadt 1900")
        .await
        .unwrap();
    assert_eq!(name_of(db, club).await, "RV Neustadt 1900");
    rename(db, &w.organizer, w.club_b, "RC Beistadt e.V.")
        .await
        .unwrap();
    assert_eq!(name_of(db, w.club_b).await, "RC Beistadt e.V.");

    assert!(matches!(
        responsible(db, &w.stranger, w.club_a, None, None).await,
        Err(RegistrationError::Forbidden)
    ));
    assert!(matches!(
        responsible(db, &w.organizer, ClubId::new(), None, None).await,
        Err(RegistrationError::NotFound)
    ));
    let before = profile(db, &w.owner_a, w.club_a).await;
    assert_eq!(before.responsible_name, before.owner_name);
    responsible(
        db,
        &w.member_a,
        w.club_a,
        Some("Erika Vorstand"),
        Some("vorstand@example.org"),
    )
    .await
    .unwrap();
    let after = profile(db, &w.owner_a, w.club_a).await;
    assert_eq!(after.responsible_name.as_deref(), Some("Erika Vorstand"));
    assert_eq!(
        after.responsible_email.as_deref(),
        Some("vorstand@example.org")
    );
    assert_eq!(after.owner_name, before.owner_name);
    responsible(db, &w.organizer, w.club_a, None, None)
        .await
        .unwrap();
    assert_eq!(profile(db, &w.owner_a, w.club_a).await, before);
}

/// `get_club` shows payments and fees only to the organizer and the club's
/// users; `list_clubs` lists every club by name with what it paid and owes.
pub async fn club_views_and_lists(db: &dyn Db) {
    let w = World::seed(db).await;
    timeplan::create_competition_with(db, |_| {}).await;
    single_act(db, &w.owner_a, w.club_a, "Anna").await;
    let treasurer = bookings::Actor::new(
        w.organizer.user_id,
        "Erika Musterfrau",
        vec![crate::roles::Role::Admin],
        None,
    );
    db.create_booking(
        &treasurer,
        &bookings::BookingInput {
            amount: 12.5,
            date: date!(2027 - 03 - 01),
            note: "Startgeld".into(),
            club_id: Some(w.club_a),
            ignored: false,
        },
    )
    .await
    .unwrap();

    assert!(matches!(
        db.get_club(
            None,
            GetClub {
                club_id: ClubId::new()
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));
    for viewer in [Some(&w.owner_a), Some(&w.member_a), Some(&w.organizer)] {
        let view = db
            .get_club(viewer, GetClub { club_id: w.club_a })
            .await
            .unwrap();
        let details = view
            .details
            .expect("details for the club and the organizer");
        assert_eq!(details.payments.len(), 1);
        assert_eq!(details.payments[0].amount, 12.5);
        assert!(details.fees.amount_due.total > 0.0);
        assert_eq!(view.profile.name, "RSV Adorf");
    }
    for viewer in [Some(&w.owner_b), Some(&w.stranger), None] {
        let view = db
            .get_club(viewer, GetClub { club_id: w.club_a })
            .await
            .unwrap();
        assert!(view.details.is_none());
        assert_eq!(view.profile.id, w.club_a);
    }

    let clubs = db.list_clubs(&w.organizer).await.unwrap();
    let names: Vec<_> = clubs.iter().map(|c| c.club.name.as_str()).collect();
    assert_eq!(names, ["RC Beistadt", "RSV Adorf"]);
    let (b, a) = (&clubs[0], &clubs[1]);
    assert_eq!(a.club.id, w.club_a);
    assert_eq!(a.club.owner_id, w.owner_a.user_id);
    assert_eq!(a.paid, 12.5);
    assert!(a.fees.amount_due.total > 0.0);
    assert_eq!(b.paid, 0.0);
    assert_eq!(b.fees.amount_due.total, 0.0);
}

/// `list_club_acts` lists the acts with a rider of the club and the groups it
/// created, with the competition's moderation flag; `edit_club_act` renames
/// and sets the description only while that flag is on; `set_act_withdrawn`
/// withdraws and takes back.
pub async fn club_acts_edit_and_withdraw(db: &dyn Db) {
    let w = World::seed(db).await;
    let act = single_act(db, &w.owner_a, w.club_a, "Anna").await;
    let group = create_group(db, &w.owner_b, w.club_b, ActType::GroupSmall, "Wirbel")
        .await
        .unwrap();

    let acts = list_acts_of(db, &w.owner_a, w.club_a).await.unwrap();
    assert!(!acts.moderation_input);
    assert_eq!(acts.acts.len(), 1);
    assert_eq!(acts.acts[0].id, act);
    assert!(!acts.acts[0].withdrawn);
    assert_eq!(acts.acts[0].participants[0].firstname, "Anna");
    let acts_b = list_acts_of(db, &w.owner_b, w.club_b).await.unwrap();
    assert_eq!(acts_b.acts.len(), 1);
    assert_eq!(acts_b.acts[0].id, group);
    assert_eq!(acts_b.acts[0].group_name.as_deref(), Some("Wirbel"));
    assert!(matches!(
        list_acts_of(db, &w.owner_b, w.club_a).await,
        Err(RegistrationError::Forbidden)
    ));
    assert!(matches!(
        list_acts_of(db, &w.organizer, ClubId::new()).await,
        Err(RegistrationError::NotFound)
    ));

    assert!(matches!(
        edit(db, &w.owner_b, act, "Kür", Some("x")).await,
        Err(RegistrationError::Forbidden)
    ));
    assert!(matches!(
        edit(db, &w.owner_a, ActId::new(), "Kür", None).await,
        Err(RegistrationError::NotFound)
    ));
    // Without the moderation input, only the name changes.
    edit(db, &w.owner_a, act, "Kür 1", Some("Beschreibung"))
        .await
        .unwrap();
    let record = &list_acts_of(db, &w.owner_a, w.club_a).await.unwrap().acts[0];
    assert_eq!(record.name, "Kür 1");
    assert_eq!(record.description, None);
    // With it, the description too.
    timeplan::create_competition_with(db, |competition| {
        competition.features.moderation_input = Some(true);
    })
    .await;
    edit(db, &w.member_a, act, "Kür 2", Some("Beschreibung"))
        .await
        .unwrap();
    let acts = list_acts_of(db, &w.owner_a, w.club_a).await.unwrap();
    assert!(acts.moderation_input);
    assert_eq!(acts.acts[0].name, "Kür 2");
    assert_eq!(acts.acts[0].description.as_deref(), Some("Beschreibung"));

    withdraw(db, &w.organizer, act, true).await.unwrap();
    assert!(list_acts_of(db, &w.owner_a, w.club_a).await.unwrap().acts[0].withdrawn);
    withdraw(db, &w.organizer, act, true).await.unwrap();
    withdraw(db, &w.organizer, act, false).await.unwrap();
    assert!(!list_acts_of(db, &w.owner_a, w.club_a).await.unwrap().acts[0].withdrawn);
    assert!(matches!(
        withdraw(db, &w.organizer, ActId::new(), true).await,
        Err(RegistrationError::NotFound)
    ));
}

/// The group commands and queries together: creating, renaming, members with
/// the type warning, the type change, and what `list_club_groups` and
/// `list_groups` show.
pub async fn group_lists_and_type_changes(db: &dyn Db) {
    let w = World::seed(db).await;
    let anna = rider(db, &w.owner_a, w.club_a, "Anna").await;
    let carla = rider(db, &w.owner_b, w.club_b, "Carla").await;
    let member = |rider_id, is_replacement| GroupMember {
        rider_id,
        is_replacement,
    };
    assert!(
        db.list_groups(&w.stranger).await.unwrap().is_empty(),
        "no groups yet"
    );

    let small = create_group(
        db,
        &w.owner_a,
        w.club_a,
        ActType::GroupSmall,
        "  Die   Wirbel ",
    )
    .await
    .unwrap();
    let large = create_group(db, &w.owner_b, w.club_b, ActType::GroupLarge, "Äther")
        .await
        .unwrap();
    assert!(
        create_group(db, &w.owner_a, w.club_a, ActType::Single, "Solo")
            .await
            .is_err()
    );
    rename_group(db, &w.owner_a, small, "Wirbelwind")
        .await
        .unwrap();
    assert_eq!(
        set_members(db, &w.owner_a, w.club_a, small, &[member(anna, false)])
            .await
            .unwrap(),
        None
    );
    set_members(db, &w.owner_b, w.club_b, small, &[member(carla, true)])
        .await
        .unwrap();

    // Anna is also in a group of the other type: no conflict. In another
    // group of the same type: the warning names her.
    assert_eq!(
        set_members(db, &w.owner_a, w.club_a, large, &[member(anna, false)])
            .await
            .unwrap(),
        None
    );
    let warning = db
        .set_club_group_type(
            &w.owner_b,
            SetClubGroupType {
                act_id: large,
                act_type: ActType::GroupSmall,
            },
        )
        .await
        .unwrap()
        .expect("Anna is now in two small groups");
    assert!(warning.contains("Anna Muster"), "{warning}");
    assert!(warning.contains("Wirbelwind"), "{warning}");
    assert_eq!(
        db.set_club_group_type(
            &w.owner_b,
            SetClubGroupType {
                act_id: large,
                act_type: ActType::GroupLarge,
            },
        )
        .await
        .unwrap(),
        None
    );
    assert!(matches!(
        db.set_club_group_type(
            &w.owner_a,
            SetClubGroupType {
                act_id: large,
                act_type: ActType::GroupLarge,
            },
        )
        .await,
        Err(RegistrationError::Forbidden)
    ));
    assert!(
        db.set_club_group_type(
            &w.owner_b,
            SetClubGroupType {
                act_id: large,
                act_type: ActType::Single,
            },
        )
        .await
        .is_err()
    );

    // All groups by name (umlauts sort like their base letter), no rider data.
    let groups = db.list_groups(&w.stranger).await.unwrap();
    let names: Vec<_> = groups.iter().map(|g| g.group_name.as_str()).collect();
    assert_eq!(names, ["Äther", "Wirbelwind"]);
    let wirbel = &groups[1];
    assert_eq!(wirbel.id, small);
    assert_eq!(wirbel.owner_club_id, Some(w.club_a));
    assert_eq!(wirbel.owner_club_name.as_deref(), Some("RSV Adorf"));
    assert_eq!((wirbel.regular_count, wirbel.replacement_count), (1, 1));
    assert_eq!(wirbel.club_names, ["RC Beistadt", "RSV Adorf"]);

    // A club sees its groups with members; birthdates only of its own riders.
    let own = db
        .list_club_groups(&w.owner_a, ListClubGroups { club_id: w.club_a })
        .await
        .unwrap();
    let names: Vec<_> = own.iter().map(|g| g.group_name.as_str()).collect();
    assert_eq!(names, ["Äther", "Wirbelwind"], "Anna is in both groups");
    let wirbel = own.iter().find(|g| g.id == small).unwrap();
    assert!(wirbel.is_owner);
    assert_eq!(wirbel.members.len(), 2);
    let members: Vec<_> = wirbel
        .members
        .iter()
        .map(|m| (m.rider_id, m.is_replacement, m.birthdate.is_some()))
        .collect();
    assert_eq!(members, [(anna, false, true), (carla, true, false)]);
    assert!(!own.iter().find(|g| g.id == large).unwrap().is_owner);
    assert!(matches!(
        db.list_club_groups(&w.owner_a, ListClubGroups { club_id: w.club_b })
            .await,
        Err(RegistrationError::Forbidden)
    ));
    assert!(matches!(
        db.list_club_groups(
            &w.organizer,
            ListClubGroups {
                club_id: ClubId::new()
            }
        )
        .await,
        Err(RegistrationError::NotFound)
    ));

    assert!(
        delete_group(db, &w.owner_b, large).await.is_err(),
        "Anna of club A is in it"
    );
    delete_group(db, &w.organizer, large).await.unwrap();
    delete_group(db, &w.organizer, small).await.unwrap();
    assert!(db.list_groups(&w.organizer).await.unwrap().is_empty());
}
