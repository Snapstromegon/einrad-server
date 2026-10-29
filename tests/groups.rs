//! Integration tests for group acts (Kleingruppe / Großgruppe): size and age
//! categories, group names, groups across clubs, the one-group-per-type rule
//! and how groups show up in lists, the startlist and the CSV export.

mod common;

use std::collections::{BTreeSet, HashMap};

use axum::http::StatusCode;
use common::{TestApp, TestResponse, rider_body};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

async fn get_act(app: &TestApp, act_id: Uuid) -> Value {
    get_act_as(app, None, act_id).await
}

async fn get_act_as(app: &TestApp, cookie: Option<&str>, act_id: Uuid) -> Value {
    let response = app
        .get(&format!("/api/query/get_act?act_id={act_id}"), cookie)
        .await;
    assert_status(&response, StatusCode::OK);
    response.body
}

async fn list_acts(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_acts", None).await;
    assert_status(&response, StatusCode::OK);
    response.body.as_array().unwrap().clone()
}

async fn list_club_acts(app: &TestApp, cookie: &str, club_id: Uuid) -> Vec<Value> {
    let response = app
        .get(
            &format!("/api/query/list_club_acts?club_id={club_id}"),
            Some(cookie),
        )
        .await;
    assert_status(&response, StatusCode::OK);
    response.body.as_array().unwrap().clone()
}

async fn list_club_riders(app: &TestApp, cookie: &str, club_id: Uuid) -> Vec<Value> {
    let response = app
        .get(
            &format!("/api/query/list_club_riders?club_id={club_id}"),
            Some(cookie),
        )
        .await;
    assert_status(&response, StatusCode::OK);
    response.body.as_array().unwrap().clone()
}

async fn list_groups(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_groups", Some(cookie)).await;
    assert_status(&response, StatusCode::OK);
    response.body.as_array().unwrap().clone()
}

async fn list_club_groups(app: &TestApp, cookie: &str, club_id: Uuid) -> Vec<Value> {
    let response = app
        .get(
            &format!("/api/query/list_club_groups?club_id={club_id}"),
            Some(cookie),
        )
        .await;
    assert_status(&response, StatusCode::OK);
    response.body.as_array().unwrap().clone()
}

/// Adds a rider without single or pair start, born on `birthdate`.
async fn add_rider_born(
    app: &TestApp,
    cookie: &str,
    club_id: Uuid,
    firstname: &str,
    birthdate: &str,
) -> Uuid {
    let mut body = rider_body(club_id, firstname, false, false);
    body["birthdate"] = json!(birthdate);
    app.add_rider(cookie, body).await
}

fn assert_status(response: &TestResponse, status: StatusCode) {
    assert_eq!(response.status, status, "{:?}", response.body);
}

fn assert_ok(response: &TestResponse) {
    assert_status(response, StatusCode::OK);
}

/// Asserts a German validation error containing `text`. The message is
/// Debug-quoted, so it's only checked with `contains`.
fn assert_error(response: &TestResponse, text: &str) {
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    let message = response.body["message"].as_str().unwrap();
    assert!(message.contains(text), "{message}");
}

/// The ids of the act's participants, in any order.
fn participant_ids(act: &Value) -> BTreeSet<String> {
    act["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect()
}

fn id_set(ids: &[Uuid]) -> BTreeSet<String> {
    ids.iter().map(Uuid::to_string).collect()
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

#[sqlx::test]
async fn the_chosen_type_stays_and_the_size_only_decides_if_it_starts(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    // Born 2015, so every group of them is U15.
    let riders = app.add_riders(&cookie, club_id, "R", 11).await;
    let small = app.create_group(&cookie, club_id, "Wirbelwind").await;
    let large = app.create_large_group(&cookie, club_id, "Sturm").await;

    let act = get_act(&app, small).await;
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(act["group_name"], "Wirbelwind");
    assert_eq!(act["owner_club_id"], club_id.to_string());
    assert_eq!(act["category"], Value::Null);
    assert_eq!(act["participants"], json!([]));
    assert_eq!(get_act(&app, large).await["act_type"], "group_large");

    // Replacement riders (Ersatzfahrer) don't count towards the size. Below 3
    // regular riders a group has no category; from 3 on it gets one of its
    // chosen type, even outside the type's size range (the frontend warns).
    for (group, act_type, regular, replacements, category) in [
        (small, "group_small", 2, 0, Value::Null),
        (small, "group_small", 2, 2, Value::Null),
        (small, "group_small", 3, 0, json!("KGU15")),
        (small, "group_small", 8, 0, json!("KGU15")),
        (small, "group_small", 9, 0, json!("KGU15")),
        (small, "group_small", 8, 2, json!("KGU15")),
        (small, "group_small", 2, 0, Value::Null),
        (large, "group_large", 2, 0, Value::Null),
        (large, "group_large", 3, 0, json!("GGU15")),
        (large, "group_large", 5, 1, json!("GGU15")),
        (large, "group_large", 11, 0, json!("GGU15")),
    ] {
        // The same riders may be in one small and one large group.
        let case = format!("{act_type}: {regular} riders + {replacements} replacements");
        let response = app
            .set_group_members(
                &cookie,
                club_id,
                group,
                &riders[..regular],
                &riders[regular..regular + replacements],
            )
            .await;
        assert_ok(&response);
        assert_eq!(response.body, json!({ "warning": null }), "{case}");

        let act = get_act(&app, group).await;
        assert_eq!(act["act_type"], act_type, "{case}");
        assert_eq!(act["category"], category, "{case}");
        let participants = act["participants"].as_array().unwrap();
        assert_eq!(participants.len(), regular + replacements, "{case}");
        let replacement_flags = participants
            .iter()
            .filter(|p| p["is_replacement"] == true)
            .count();
        assert_eq!(replacement_flags, replacements, "{case}");
    }
}

#[sqlx::test]
async fn a_group_is_a_small_or_a_large_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let group = app.create_group(&cookie, club_id, "Wirbelwind").await;

    for act_type in ["single", "pair"] {
        let response = app
            .post(
                "/api/command/create_club_group",
                json!({ "club_id": club_id, "group_name": "Falsch", "act_type": act_type }),
                Some(&cookie),
            )
            .await;
        assert_error(&response, "Kleingruppe oder eine Großgruppe");
        let response = app
            .post(
                "/api/command/set_club_group_type",
                json!({ "act_id": group, "act_type": act_type }),
                Some(&cookie),
            )
            .await;
        assert_error(&response, "Kleingruppe oder eine Großgruppe");
    }
    // The type has to be chosen.
    let response = app
        .post(
            "/api/command/create_club_group",
            json!({ "club_id": club_id, "group_name": "Ohne Art" }),
            Some(&cookie),
        )
        .await;
    assert_status(&response, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(list_groups(&app, &cookie).await.len(), 1);
    assert_eq!(get_act(&app, group).await["act_type"], "group_small");
}

#[sqlx::test]
async fn only_the_owner_club_or_an_admin_changes_the_group_type(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let admin = app.create_admin().await;
    let a = app.add_riders(&owner, club_id, "A", 2).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 3).await;
    let group = app.create_group(&owner, club_id, "Gemeinsam").await;
    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b, &[])
            .await,
    );
    let set_type = |act_type: &'static str| json!({ "act_id": group, "act_type": act_type });

    let response = app
        .post(
            "/api/command/set_club_group_type",
            set_type("group_large"),
            None,
        )
        .await;
    assert_status(&response, StatusCode::UNAUTHORIZED);
    // A club with riders in the group, but not its owner.
    let response = app
        .post(
            "/api/command/set_club_group_type",
            set_type("group_large"),
            Some(&guest),
        )
        .await;
    assert_status(&response, StatusCode::FORBIDDEN);
    assert_eq!(get_act(&app, group).await["category"], "KGU15");

    let response = app
        .post(
            "/api/command/set_club_group_type",
            set_type("group_large"),
            Some(&owner),
        )
        .await;
    assert_ok(&response);
    assert_eq!(response.body, json!({ "warning": null }));
    let act = get_act(&app, group).await;
    assert_eq!(act["act_type"], "group_large");
    assert_eq!(act["category"], "GGU15");
    // Setting the same type again changes nothing.
    assert_ok(
        &app.post(
            "/api/command/set_club_group_type",
            set_type("group_large"),
            Some(&owner),
        )
        .await,
    );

    let response = app
        .post(
            "/api/command/set_club_group_type",
            set_type("group_small"),
            Some(&admin),
        )
        .await;
    assert_ok(&response);
    let act = get_act(&app, group).await;
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(act["category"], "KGU15");
}

#[sqlx::test]
async fn oldest_rider_decides_age_group_including_replacements(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let admin = app.create_admin().await;
    let young = app.add_riders(&cookie, club_id, "Jung", 4).await;
    // 15 on the determining day 2027-06-01 …
    let olga = add_rider_born(&app, &cookie, club_id, "Olga", "2012-06-01T00:00:00.000Z").await;
    // … and one day too young for 15+.
    let yara = add_rider_born(&app, &cookie, club_id, "Yara", "2012-06-02T00:00:00.000Z").await;
    let sepp = add_rider_born(&app, &cookie, club_id, "Sepp", "1990-01-01T00:00:00.000Z").await;

    let older = app.create_group(&cookie, club_id, "Ältere").await;
    let response = app
        .set_group_members(&cookie, club_id, older, &[young[0], young[1], olga], &[])
        .await;
    assert_ok(&response);
    assert_eq!(get_act(&app, older).await["category"], "KG15+");

    let younger = app.create_group(&cookie, club_id, "Jüngere").await;
    let regular = [young[2], young[3], yara];
    assert_ok(
        &app.set_group_members(&cookie, club_id, younger, &regular, &[])
            .await,
    );
    let act = get_act_as(&app, Some(&admin), younger).await;
    assert_eq!(act["category"], "KGU15");
    let max_age = act["max_age"].as_f64().unwrap();
    assert!((14.99..15.0).contains(&max_age), "{max_age}");
    // Clubs only see the age in whole years: the exact age would give away
    // the oldest rider's birthdate.
    let groups = list_club_groups(&app, &cookie, club_id).await;
    let group = groups
        .iter()
        .find(|g| g["id"] == younger.to_string())
        .unwrap();
    assert_eq!(group["max_age"], 14.0);

    // Replacements may ride in the end, so an older replacement counts.
    assert_ok(
        &app.set_group_members(&cookie, club_id, younger, &regular, &[sepp])
            .await,
    );
    let act = get_act_as(&app, Some(&admin), younger).await;
    assert_eq!(act["category"], "KG15+");
    assert!(act["max_age"].as_f64().unwrap() > 37.0, "{act}");
}

#[sqlx::test]
async fn group_names_are_unique_ignoring_case(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (other, other_club_id) = app.create_club_owner("Other", "SSV").await;
    let group = app.create_group(&owner, club_id, "Wirbelwind").await;

    for (cookie, club, name) in [
        (&owner, club_id, "Wirbelwind"),
        (&owner, club_id, "wirbelwind"),
        (&other, other_club_id, "WIRBELWIND"),
        (&other, other_club_id, "  Wirbelwind "),
    ] {
        let response = app
            .post(
                "/api/command/create_club_group",
                json!({ "act_type": "group_small", "club_id": club, "group_name": name }),
                Some(cookie),
            )
            .await;
        assert_error(&response, "ist bereits vergeben");
    }
    let response = app
        .post(
            "/api/command/create_club_group",
            json!({ "act_type": "group_small", "club_id": club_id, "group_name": "   " }),
            Some(&owner),
        )
        .await;
    assert_error(&response, "Gruppennamen");

    // Names are stored trimmed, with single spaces.
    let other_group = app
        .create_group(&other, other_club_id, "  Rollende   Rebellen ")
        .await;
    assert_eq!(
        get_act(&app, other_group).await["group_name"],
        "Rollende Rebellen"
    );

    // Renaming can't take another group's name either …
    let response = app
        .post(
            "/api/command/rename_club_group",
            json!({ "act_id": other_group, "group_name": "WIRBELWIND" }),
            Some(&other),
        )
        .await;
    assert_error(&response, "ist bereits vergeben");
    // … but can change the case of its own.
    let response = app
        .post(
            "/api/command/rename_club_group",
            json!({ "act_id": group, "group_name": "WirbelWind" }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);

    // Case is ignored beyond ASCII letters, too.
    app.create_group(&owner, club_id, "Überflieger").await;
    for name in ["überflieger", "ÜBERFLIEGER"] {
        let response = app
            .post(
                "/api/command/create_club_group",
                json!({ "act_type": "group_small", "club_id": other_club_id, "group_name": name }),
                Some(&other),
            )
            .await;
        assert_error(&response, "ist bereits vergeben");
    }
    let response = app
        .post(
            "/api/command/rename_club_group",
            json!({ "act_id": other_group, "group_name": "überflieger" }),
            Some(&other),
        )
        .await;
    assert_error(&response, "ist bereits vergeben");
    // Names spreadsheets would read as formulas are rejected.
    let response = app
        .post(
            "/api/command/create_club_group",
            json!({ "act_type": "group_small", "club_id": club_id, "group_name": "=HYPERLINK(\"http://x\";\"Klick\")" }),
            Some(&owner),
        )
        .await;
    assert_error(&response, "unerlaubte Zeichen");

    // Umlauts are sorted like their base letter.
    let names: Vec<_> = list_groups(&app, &owner)
        .await
        .iter()
        .map(|g| g["group_name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Rollende Rebellen", "Überflieger", "WirbelWind"]);
    let names: Vec<_> = list_club_groups(&app, &owner, club_id)
        .await
        .iter()
        .map(|g| g["group_name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Überflieger", "WirbelWind"]);
}

#[sqlx::test]
async fn clubs_only_change_their_own_riders_in_a_shared_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let a = app.add_riders(&owner, club_id, "A", 3).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 2).await;
    let group = app.create_group(&owner, club_id, "Gemeinsam").await;

    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    // Every club can join every group, with its own riders.
    let response = app
        .set_group_members(&guest, guest_club_id, group, &b, &[])
        .await;
    assert_ok(&response);
    assert_eq!(response.body, json!({ "warning": null }));
    let all: Vec<Uuid> = a.iter().chain(&b).copied().collect();
    assert_eq!(participant_ids(&get_act(&app, group).await), id_set(&all));

    // Setting a club's riders keeps the riders of other clubs.
    assert_ok(
        &app.set_group_members(&owner, club_id, group, &a[..1], &[])
            .await,
    );
    let remaining = [a[0], b[0], b[1]];
    assert_eq!(
        participant_ids(&get_act(&app, group).await),
        id_set(&remaining)
    );

    // Riders of another club can't be added …
    let response = app
        .set_group_members(&owner, club_id, group, &[a[0], b[0]], &[])
        .await;
    assert_error(&response, "nur seine eigenen FahrerInnen");
    // … and other clubs' riders can't be changed on their behalf.
    let response = app
        .set_group_members(&owner, guest_club_id, group, &[], &[])
        .await;
    assert_status(&response, StatusCode::FORBIDDEN);
    let response = app
        .set_group_members(&owner, Uuid::now_v7(), group, &[], &[])
        .await;
    assert_status(&response, StatusCode::NOT_FOUND);
    // A rider can be in the group only once.
    let response = app
        .set_group_members(&owner, club_id, group, &[a[0]], &[a[0]])
        .await;
    assert_error(&response, "nur einmal");
    // Only groups can be joined.
    let single = app.add_single_act(&owner, club_id, "Solo").await;
    for act_id in [single, Uuid::now_v7()] {
        let response = app
            .set_group_members(&owner, club_id, act_id, &a[1..2], &[])
            .await;
        assert_status(&response, StatusCode::NOT_FOUND);
    }
    let response = app
        .post(
            "/api/command/set_club_group_members",
            json!({ "club_id": club_id, "act_id": group, "members": [] }),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let response = app
        .post(
            "/api/command/create_club_group",
            json!({ "act_type": "group_small", "club_id": club_id, "group_name": "Ohne Login" }),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    // A club can't create groups for another club.
    let response = app
        .post(
            "/api/command/create_club_group",
            json!({ "act_type": "group_small", "club_id": guest_club_id, "group_name": "Fremd" }),
            Some(&owner),
        )
        .await;
    assert_status(&response, StatusCode::FORBIDDEN);

    // None of the rejected changes was stored.
    assert_eq!(
        participant_ids(&get_act(&app, group).await),
        id_set(&remaining)
    );

    // An empty list means the club leaves the group.
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &[], &[])
            .await,
    );
    assert_eq!(
        participant_ids(&get_act(&app, group).await),
        id_set(&a[..1])
    );
}

#[sqlx::test]
async fn only_the_owner_club_or_an_admin_renames_and_deletes(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let loner = app.cookie_for(app.create_user("Loner", false).await);
    let admin = app.create_admin().await;
    let group = app.create_group(&owner, club_id, "Gemeinsam").await;
    let b = app.add_riders(&guest, guest_club_id, "B", 1).await;
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b, &[])
            .await,
    );

    // Joining a group doesn't make a club its owner.
    for cookie in [&guest, &loner] {
        let response = app
            .post(
                "/api/command/rename_club_group",
                json!({ "act_id": group, "group_name": "Gekapert" }),
                Some(cookie),
            )
            .await;
        assert_status(&response, StatusCode::FORBIDDEN);
        let response = app
            .post(
                "/api/command/delete_club_group",
                json!({ "act_id": group }),
                Some(cookie),
            )
            .await;
        assert_status(&response, StatusCode::FORBIDDEN);
    }
    assert_eq!(get_act(&app, group).await["group_name"], "Gemeinsam");

    for (cookie, name) in [(&owner, "Neuer Name"), (&admin, "Vom Admin")] {
        let response = app
            .post(
                "/api/command/rename_club_group",
                json!({ "act_id": group, "group_name": name }),
                Some(cookie),
            )
            .await;
        assert_ok(&response);
        assert_eq!(get_act(&app, group).await["group_name"], name);
    }

    // Unknown acts and acts that aren't groups can't be renamed or deleted.
    let single = app.add_single_act(&owner, club_id, "Solo").await;
    for act_id in [single, Uuid::now_v7()] {
        let response = app
            .post(
                "/api/command/rename_club_group",
                json!({ "act_id": act_id, "group_name": "X" }),
                Some(&admin),
            )
            .await;
        assert_status(&response, StatusCode::NOT_FOUND);
        let response = app
            .post(
                "/api/command/delete_club_group",
                json!({ "act_id": act_id }),
                Some(&admin),
            )
            .await;
        assert_status(&response, StatusCode::NOT_FOUND);
    }

    // The owner deletes the group once it has no other clubs' riders.
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &[], &[])
            .await,
    );
    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);
    assert!(list_groups(&app, &owner).await.is_empty());
}

#[sqlx::test]
async fn owner_cannot_delete_a_group_while_other_clubs_have_riders(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let admin = app.create_admin().await;
    let a = app.add_riders(&owner, club_id, "A", 2).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 2).await;

    let group = app.create_group(&owner, club_id, "Gemeinsam").await;
    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b[..1], &[])
            .await,
    );
    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&owner),
        )
        .await;
    assert_error(&response, "FahrerInnen anderer Vereine");
    assert_eq!(participant_ids(&get_act(&app, group).await).len(), 3);

    // Once the other club left, the owner can delete it.
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &[], &[])
            .await,
    );
    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);

    // Admins can delete a group with riders of several clubs.
    let group = app.create_group(&owner, club_id, "Zweiter Versuch").await;
    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b, &[])
            .await,
    );
    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&admin),
        )
        .await;
    assert_ok(&response);
    assert!(list_groups(&app, &owner).await.is_empty());
    assert_eq!(list_club_riders(&app, &guest, guest_club_id).await.len(), 2);
}

#[sqlx::test]
async fn a_rider_starts_in_only_one_small_group_even_as_replacement(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let r = app.add_riders(&cookie, club_id, "R", 6).await;
    let first = app.create_group(&cookie, club_id, "Erste").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, first, &r[..3], &[])
            .await,
    );
    let second = app.create_group(&cookie, club_id, "Zweite").await;

    for (regular, replacements) in [
        (vec![r[3], r[4], r[0]], vec![]),
        (r[3..6].to_vec(), vec![r[0]]),
    ] {
        // Saved with a warning; neither group starts until it's sorted out.
        let response = app
            .set_group_members(&cookie, club_id, second, &regular, &replacements)
            .await;
        assert_ok(&response);
        let warning = response.body["warning"].as_str().unwrap();
        assert!(warning.starts_with("Kleingruppe „Zweite“:"), "{warning}");
        assert!(
            warning
                .contains("„R1 Muster“ (RSV) ist bereits in der Kleingruppe „Erste“ angemeldet."),
            "{warning}"
        );
        assert!(warning.contains("auch als ErsatzfahrerIn"), "{warning}");
        for group in [first, second] {
            let act = get_act(&app, group).await;
            assert_eq!(act["category"], Value::Null, "{act}");
            assert_eq!(act["type_conflict"], true, "{act}");
        }
        let groups = list_club_groups(&app, &cookie, club_id).await;
        assert_eq!(groups.len(), 2);
        for group in &groups {
            let conflict = group["conflict"].as_str().unwrap();
            assert!(conflict.contains("„R1 Muster“ (RSV)"), "{conflict}");
            assert_eq!(group["category"], Value::Null);
        }
    }
    let response = app.get("/api/query/startlist", None).await;
    assert_eq!(response.body, json!([]));

    let response = app
        .set_group_members(&cookie, club_id, second, &r[3..6], &[])
        .await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    for group in [first, second] {
        let act = get_act(&app, group).await;
        assert_eq!(act["category"], "KGU15");
        assert_eq!(act["type_conflict"], false);
    }
    let groups = list_club_groups(&app, &cookie, club_id).await;
    assert!(groups.iter().all(|g| g["conflict"].is_null()), "{groups:?}");
}

#[sqlx::test]
async fn a_rider_starts_in_one_small_and_one_large_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let r = app.add_riders(&cookie, club_id, "R", 9).await;
    let s = app.add_riders(&cookie, club_id, "S", 8).await;

    let small = app.create_group(&cookie, club_id, "Klein").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, small, &r[..3], &[])
            .await,
    );
    let large = app.create_large_group(&cookie, club_id, "Groß").await;
    let response = app
        .set_group_members(&cookie, club_id, large, &r, &[])
        .await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    assert_eq!(get_act(&app, small).await["category"], "KGU15");
    assert_eq!(get_act(&app, large).await["category"], "GGU15");

    // But not in a second large group.
    let larger = app
        .create_large_group(&cookie, club_id, "Noch größer")
        .await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, larger, &s, &[])
            .await,
    );
    let mut regular = s.clone();
    regular.push(r[0]);
    let response = app
        .set_group_members(&cookie, club_id, larger, &regular, &[])
        .await;
    assert_ok(&response);
    let warning = response.body["warning"].as_str().unwrap();
    assert!(
        warning.starts_with("Großgruppe „Noch größer“:"),
        "{warning}"
    );
    assert!(
        warning.contains("„R1 Muster“ (RSV) ist bereits in der Großgruppe „Groß“ angemeldet."),
        "{warning}"
    );
    for group in [large, larger] {
        let act = get_act(&app, group).await;
        assert_eq!(act["category"], Value::Null, "{act}");
        assert_eq!(act["type_conflict"], true, "{act}");
    }
    // The small group isn't affected.
    let act = get_act(&app, small).await;
    assert_eq!(act["category"], "KGU15");
    assert_eq!(act["type_conflict"], false);
}

#[sqlx::test]
async fn changing_the_type_warns_if_a_rider_is_in_another_group_of_it(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;

    let b = app.add_riders(&guest, guest_club_id, "B", 9).await;
    let alpha = app.create_large_group(&guest, guest_club_id, "Alpha").await;
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, alpha, &b, &[])
            .await,
    );

    // B1 of the large group Alpha also joins the small group Beta. Growing to
    // 9 regular riders doesn't make Beta a large group.
    let a = app.add_riders(&owner, club_id, "A", 8).await;
    let beta = app.create_group(&owner, club_id, "Beta").await;
    assert_ok(&app.set_group_members(&owner, club_id, beta, &a, &[]).await);
    let response = app
        .set_group_members(&guest, guest_club_id, beta, &b[..1], &[])
        .await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    let act = get_act(&app, beta).await;
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(act["category"], "KGU15");

    // Its owner makes it a large group: a second one for B1.
    let response = app
        .post(
            "/api/command/set_club_group_type",
            json!({ "act_id": beta, "act_type": "group_large" }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);
    let warning = response.body["warning"].as_str().unwrap();
    assert!(warning.starts_with("Großgruppe „Beta“:"), "{warning}");
    assert!(
        warning.contains("„B1 Muster“ (SSV) ist bereits in der Großgruppe „Alpha“ angemeldet."),
        "{warning}"
    );
    for group in [alpha, beta] {
        let act = get_act(&app, group).await;
        assert_eq!(act["category"], Value::Null, "{act}");
        assert_eq!(act["type_conflict"], true, "{act}");
    }
    // The other club sees the conflict in both groups.
    let guest_groups = list_club_groups(&app, &guest, guest_club_id).await;
    assert_eq!(guest_groups.len(), 2);
    for group in &guest_groups {
        let conflict = group["conflict"].as_str().unwrap();
        assert!(conflict.contains("„B1 Muster“ (SSV)"), "{conflict}");
    }

    // Back to a small group, both start again.
    let response = app
        .post(
            "/api/command/set_club_group_type",
            json!({ "act_id": beta, "act_type": "group_small" }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    assert_eq!(get_act(&app, beta).await["category"], "KGU15");
    assert_eq!(get_act(&app, alpha).await["category"], "GGU15");
}

#[sqlx::test]
async fn a_shared_large_group_can_be_built_one_club_at_a_time(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (rsv, rsv_id) = app.create_club_owner("Owner", "RSV").await;
    let (ssv, ssv_id) = app.create_club_owner("Guest", "SSV").await;
    let a = app.add_riders(&rsv, rsv_id, "A", 5).await;
    let b = app.add_riders(&ssv, ssv_id, "B", 5).await;

    // Every rider starts in their club's small group …
    let rsv_small = app.create_group(&rsv, rsv_id, "RSV klein").await;
    assert_ok(
        &app.set_group_members(&rsv, rsv_id, rsv_small, &a, &[])
            .await,
    );
    let ssv_small = app.create_group(&ssv, ssv_id, "SSV klein").await;
    assert_ok(
        &app.set_group_members(&ssv, ssv_id, ssv_small, &b, &[])
            .await,
    );

    // … and the clubs build a large group together, one after another. With
    // the first club's riders only, it is still a large group: it doesn't
    // block their small group, and it's planned already.
    let together = app.create_large_group(&ssv, ssv_id, "NRW Auswahl").await;
    let response = app.set_group_members(&ssv, ssv_id, together, &b, &[]).await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    for (group, category) in [
        (rsv_small, "KGU15"),
        (ssv_small, "KGU15"),
        (together, "GGU15"),
    ] {
        assert_eq!(get_act(&app, group).await["category"], category);
    }
    let response = app.get("/api/query/startlist", None).await;
    assert_eq!(response.body.as_array().unwrap().len(), 3);

    let response = app.set_group_members(&rsv, rsv_id, together, &a, &[]).await;
    assert_ok(&response);
    assert_eq!(response.body["warning"], Value::Null);
    let act = get_act(&app, together).await;
    assert_eq!(act["act_type"], "group_large");
    assert_eq!(act["category"], "GGU15");
    assert_eq!(act["participants"].as_array().unwrap().len(), 10);
    for group in [rsv_small, ssv_small, together] {
        assert_eq!(get_act(&app, group).await["type_conflict"], false);
    }
    for (cookie, club_id) in [(&rsv, rsv_id), (&ssv, ssv_id)] {
        let groups = list_club_groups(&app, cookie, club_id).await;
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|g| g["conflict"].is_null()), "{groups:?}");
    }
}

#[sqlx::test]
async fn deleting_a_rider_only_removes_them_from_the_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let mut riders = app.add_riders(&cookie, club_id, "R", 8).await;
    let solo = app
        .add_rider(&cookie, rider_body(club_id, "Solo", true, false))
        .await;
    riders.push(solo);
    let group = app.create_large_group(&cookie, club_id, "Neun").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, group, &riders, &[])
            .await,
    );
    assert_eq!(list_acts(&app).await.len(), 2);

    let response = app
        .post(
            "/api/command/delete_rider",
            json!({ "rider_id": solo }),
            Some(&cookie),
        )
        .await;
    assert_ok(&response);
    assert_eq!(response.body, json!({}));

    // The group keeps its other riders and stays a large group.
    let act = get_act(&app, group).await;
    assert_eq!(act["act_type"], "group_large");
    assert_eq!(act["category"], "GGU15");
    assert_eq!(participant_ids(&act), id_set(&riders[..8]));
    assert_eq!(list_club_riders(&app, &cookie, club_id).await.len(), 8);
    // The rider's single act is gone with them.
    let acts = list_acts(&app).await;
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0]["id"], group.to_string());
}

#[sqlx::test]
async fn deleting_a_rider_leaves_their_small_and_large_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let r = app.add_riders(&cookie, club_id, "R", 10).await;
    let small = app.create_group(&cookie, club_id, "Klein").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, small, &[r[0], r[8], r[9]], &[])
            .await,
    );
    let large = app.create_large_group(&cookie, club_id, "Groß").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, large, &r[..9], &[])
            .await,
    );

    let response = app
        .post(
            "/api/command/delete_rider",
            json!({ "rider_id": r[8] }),
            Some(&cookie),
        )
        .await;
    assert_ok(&response);

    // R9 left both groups and is gone. Both keep their type, so no rider is
    // in two groups of the same type: Groß still starts with 8 riders, Klein
    // is incomplete with 2.
    let act = get_act(&app, small).await;
    assert_eq!(participant_ids(&act), id_set(&[r[0], r[9]]));
    assert_eq!(act["category"], Value::Null);
    let act = get_act(&app, large).await;
    assert_eq!(act["act_type"], "group_large");
    assert_eq!(act["category"], "GGU15");
    assert_eq!(act["type_conflict"], false);
    assert_eq!(participant_ids(&act), id_set(&r[..8]));
    let riders = list_club_riders(&app, &cookie, club_id).await;
    assert_eq!(riders.len(), 9);
    assert!(riders.iter().all(|s| s["id"] != r[8].to_string()));
}

#[sqlx::test]
async fn other_clubs_cannot_remove_or_change_riders_of_a_shared_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let a = app.add_riders(&owner, club_id, "A", 2).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 1).await;
    let group = app.create_group(&owner, club_id, "Gemeinsam").await;
    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b, &[])
            .await,
    );
    let everyone = id_set(&[a[0], a[1], b[0]]);

    let response = app
        .post(
            "/api/command/delete_rider",
            json!({ "rider_id": a[0] }),
            Some(&guest),
        )
        .await;
    assert_status(&response, StatusCode::FORBIDDEN);
    let mut older = rider_body(club_id, "A1", false, false);
    older.as_object_mut().unwrap().remove("club_id");
    older["rider_id"] = json!(a[0]);
    older["birthdate"] = json!("1990-01-01T00:00:00.000Z");
    let response = app
        .post("/api/command/edit_rider", older, Some(&guest))
        .await;
    assert_status(&response, StatusCode::FORBIDDEN);

    let act = get_act(&app, group).await;
    assert_eq!(participant_ids(&act), everyone);
    assert_eq!(act["category"], "KGU15");

    // A club deletes its own rider of the shared group.
    let response = app
        .post(
            "/api/command/delete_rider",
            json!({ "rider_id": b[0] }),
            Some(&guest),
        )
        .await;
    assert_ok(&response);
    let act = get_act(&app, group).await;
    assert_eq!(participant_ids(&act), id_set(&a));
    assert_eq!(act["category"], Value::Null);
}

#[sqlx::test]
async fn a_new_birthdate_moves_the_group_to_another_age_group(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let r = app.add_riders(&cookie, club_id, "R", 3).await;
    let group = app.create_group(&cookie, club_id, "Trio").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, group, &r, &[])
            .await,
    );
    assert_eq!(get_act(&app, group).await["category"], "KGU15");

    for (birthdate, category) in [
        ("1990-01-01T00:00:00.000Z", "KG15+"),
        ("2015-05-01T00:00:00.000Z", "KGU15"),
    ] {
        let mut body = rider_body(club_id, "R1", false, false);
        body.as_object_mut().unwrap().remove("club_id");
        body["rider_id"] = json!(r[0]);
        body["birthdate"] = json!(birthdate);
        let response = app
            .post("/api/command/edit_rider", body, Some(&cookie))
            .await;
        assert_ok(&response);

        // Same act, same riders, only the category changes.
        let acts = list_acts(&app).await;
        assert_eq!(acts.len(), 1, "{acts:?}");
        assert_eq!(acts[0]["id"], group.to_string());
        assert_eq!(acts[0]["category"], category);
        assert_eq!(participant_ids(&acts[0]), id_set(&r));
    }
}

#[sqlx::test]
async fn deleting_a_group_keeps_its_riders(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let mut riders = app.add_riders(&cookie, club_id, "R", 2).await;
    let solo = app
        .add_rider(&cookie, rider_body(club_id, "Solo", true, false))
        .await;
    riders.push(solo);
    let group = app.create_group(&cookie, club_id, "Kurzlebig").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, group, &riders, &[])
            .await,
    );

    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&cookie),
        )
        .await;
    assert_ok(&response);

    let riders = list_club_riders(&app, &cookie, club_id).await;
    assert_eq!(riders.len(), 3);
    assert!(riders.iter().all(|s| s["group_count"] == 0), "{riders:?}");
    let acts = list_acts(&app).await;
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0]["act_type"], "single");

    let response = app
        .post(
            "/api/command/delete_club_group",
            json!({ "act_id": group }),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn single_pair_and_group_starts_of_a_rider_are_all_listed(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app
        .add_rider(&cookie, rider_body(club_id, "Anna", true, true))
        .await;
    let mut berta = rider_body(club_id, "Berta", false, true);
    berta["partner_id"] = json!(anna);
    let berta = app.add_rider(&cookie, berta).await;
    let carla = app
        .add_rider(&cookie, rider_body(club_id, "Carla", false, false))
        .await;
    let group = app.create_group(&cookie, club_id, "Trio").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, group, &[anna, berta, carla], &[])
            .await,
    );

    let acts = list_acts(&app).await;
    let anna_id = anna.to_string();
    let anna_acts: BTreeSet<_> = acts
        .iter()
        .filter(|act| participant_ids(act).contains(&anna_id))
        .map(|act| act["act_type"].as_str().unwrap())
        .collect();
    assert_eq!(anna_acts, BTreeSet::from(["group_small", "pair", "single"]));

    let group_act = acts
        .iter()
        .find(|act| act["id"] == group.to_string())
        .unwrap();
    assert_eq!(group_act["group_name"], "Trio");
    assert_eq!(group_act["owner_club_id"], club_id.to_string());
    assert_eq!(group_act["category"], "KGU15");
    for participant in group_act["participants"].as_array().unwrap() {
        assert_eq!(participant["club_id"], club_id.to_string());
        assert_eq!(participant["club_name"], "RSV");
        assert_eq!(participant["is_replacement"], false);
    }
    // Singles and pairs have no group fields.
    let single = acts.iter().find(|act| act["act_type"] == "single").unwrap();
    assert_eq!(single["group_name"], Value::Null);
    assert_eq!(single["owner_club_id"], Value::Null);

    let club_acts = list_club_acts(&app, &cookie, club_id).await;
    assert_eq!(club_acts.len(), 3);
    let club_group = club_acts
        .iter()
        .find(|act| act["id"] == group.to_string())
        .unwrap();
    assert_eq!(club_group["group_name"], "Trio");
    assert_eq!(club_group["participants"].as_array().unwrap().len(), 3);

    let riders = list_club_riders(&app, &cookie, club_id).await;
    let anna = riders.iter().find(|s| s["id"] == anna_id).unwrap();
    // The starts come from the acts; riders don't carry them.
    assert_eq!(anna["single_female"], Value::Null);
    assert_eq!(anna["pair"], Value::Null);
    assert_eq!(anna["group_count"], 1);
}

#[sqlx::test]
async fn groups_are_listed_without_rider_data_and_per_club_with_members(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let loner = app.cookie_for(app.create_user("Loner", false).await);
    let admin = app.create_admin().await;
    let a = app.add_riders(&owner, club_id, "A", 4).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 1).await;
    let alpha = app.create_group(&owner, club_id, "Alpha").await;
    assert_ok(
        &app.set_group_members(&owner, club_id, alpha, &a[..3], &a[3..])
            .await,
    );
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, alpha, &b, &[])
            .await,
    );
    let empty = app.create_group(&owner, club_id, "Leer").await;

    // Every logged-in user sees all groups, without any rider data.
    let response = app.get("/api/query/list_groups", None).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let groups = list_groups(&app, &loner).await;
    assert_eq!(groups.len(), 2);
    assert_eq!(
        keys(&groups[0]),
        BTreeSet::from([
            "act_type",
            "category",
            "club_names",
            "group_name",
            "id",
            "owner_club_id",
            "owner_club_name",
            "regular_count",
            "replacement_count",
        ])
    );
    assert_eq!(
        groups[0],
        json!({
            "id": alpha,
            "group_name": "Alpha",
            "act_type": "group_small",
            "category": "KGU15",
            "owner_club_id": club_id,
            "owner_club_name": "RSV",
            "regular_count": 4,
            "replacement_count": 1,
            "club_names": ["RSV", "SSV"],
        })
    );
    assert_eq!(groups[1]["id"], empty.to_string());
    assert_eq!(groups[1]["category"], Value::Null);
    assert_eq!(groups[1]["regular_count"], 0);
    assert_eq!(groups[1]["club_names"], json!([]));

    // A club sees the groups it owns or has riders in, with all members, but
    // only the birthdates of its own riders.
    let guest_groups = list_club_groups(&app, &guest, guest_club_id).await;
    assert_eq!(guest_groups.len(), 1);
    let group = &guest_groups[0];
    assert_eq!(
        keys(group),
        BTreeSet::from([
            "act_type",
            "category",
            "conflict",
            "group_name",
            "id",
            "is_owner",
            "max_age",
            "members",
            "name",
            "owner_club_id",
            "owner_club_name",
            "song_file",
        ])
    );
    assert_eq!(group["id"], alpha.to_string());
    assert_eq!(group["is_owner"], false);
    assert_eq!(group["owner_club_name"], "RSV");
    assert_eq!(group["category"], "KGU15");
    assert_eq!(group["conflict"], Value::Null);
    // Whole years only; other clubs' birthdates stay hidden.
    assert_eq!(group["max_age"].as_f64().unwrap().fract(), 0.0);
    let members = group["members"].as_array().unwrap();
    // Regular riders first, then replacements, each by name.
    let order: Vec<_> = members
        .iter()
        .map(|m| {
            (
                m["firstname"].as_str().unwrap(),
                m["is_replacement"] == true,
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("A1", false),
            ("A2", false),
            ("A3", false),
            ("B1", false),
            ("A4", true)
        ]
    );
    for member in members {
        let own = member["club_id"] == guest_club_id.to_string();
        assert_eq!(member["club_name"], if own { "SSV" } else { "RSV" });
        assert_eq!(member["birthdate"].is_string(), own, "{member}");
    }

    let owner_groups = list_club_groups(&app, &owner, club_id).await;
    let names: Vec<_> = owner_groups.iter().map(|g| &g["group_name"]).collect();
    assert_eq!(names, ["Alpha", "Leer"]);
    assert!(owner_groups.iter().all(|g| g["is_owner"] == true));
    for member in owner_groups[0]["members"].as_array().unwrap() {
        let own = member["club_id"] == club_id.to_string();
        assert_eq!(member["birthdate"].is_string(), own, "{member}");
    }
    assert_eq!(owner_groups[1]["members"], json!([]));

    // Clubs only list their own groups; admins any club's.
    let response = app
        .get(
            &format!("/api/query/list_club_groups?club_id={club_id}"),
            Some(&guest),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    let response = app
        .get(
            &format!("/api/query/list_club_groups?club_id={club_id}"),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let response = app
        .get(
            &format!("/api/query/list_club_groups?club_id={}", Uuid::now_v7()),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(list_club_groups(&app, &admin, guest_club_id).await.len(), 1);

    // The club's acts hide other clubs' birthdates as well, and include owned
    // groups without riders.
    let guest_acts = list_club_acts(&app, &guest, guest_club_id).await;
    assert_eq!(guest_acts.len(), 1);
    for participant in guest_acts[0]["participants"].as_array().unwrap() {
        let own = participant["club_id"] == guest_club_id.to_string();
        assert_eq!(participant["birthdate"].is_string(), own, "{participant}");
    }
    let owner_acts = list_club_acts(&app, &owner, club_id).await;
    assert!(owner_acts.iter().any(|act| act["id"] == empty.to_string()));
}

#[sqlx::test]
async fn group_count_counts_group_starts_including_replacements(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let admin = app.create_admin().await;
    let r = app.add_riders(&cookie, club_id, "R", 9).await;
    let solo = app
        .add_rider(&cookie, rider_body(club_id, "Solo", true, false))
        .await;
    let small = app.create_group(&cookie, club_id, "Klein").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, small, &r[..3], &r[3..4])
            .await,
    );
    let large = app.create_large_group(&cookie, club_id, "Groß").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, large, &r, &[])
            .await,
    );

    let expected = HashMap::from([
        (r[0].to_string(), 2),
        // Replacement in Klein, regular rider in Groß.
        (r[3].to_string(), 2),
        (r[4].to_string(), 1),
        (solo.to_string(), 0),
    ]);
    let club_riders = list_club_riders(&app, &cookie, club_id).await;
    let response = app.get("/api/query/list_riders", Some(&admin)).await;
    assert_status(&response, StatusCode::OK);
    let all_riders = response.body.as_array().unwrap().clone();
    for riders in [club_riders, all_riders] {
        assert_eq!(riders.len(), 10);
        for (id, count) in &expected {
            let rider = riders.iter().find(|s| s["id"] == *id).unwrap();
            assert_eq!(rider["group_count"], *count, "{rider}");
        }
    }
}

#[sqlx::test]
async fn startlist_and_csv_show_groups_by_name(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club_id) = app.create_club_owner("Guest", "SSV").await;
    let a = app.add_riders(&owner, club_id, "A", 2).await;
    let b = app.add_riders(&guest, guest_club_id, "B", 2).await;
    // ';' separates the CSV's columns.
    let group = app.create_group(&owner, club_id, "Wind;Wetter").await;
    assert_ok(&app.set_group_members(&owner, club_id, group, &a, &[]).await);
    assert_ok(
        &app.set_group_members(&guest, guest_club_id, group, &b[..1], &b[1..])
            .await,
    );
    // Spreadsheets would read this title as a formula.
    let response = app
        .post(
            "/api/command/edit_club_act",
            json!({ "id": group, "name": "=Sturmfahrt", "description": null }),
            Some(&owner),
        )
        .await;
    assert_ok(&response);
    // Incomplete groups have no category, so they don't start (yet).
    let c = app.add_riders(&owner, club_id, "C", 2).await;
    let incomplete = app.create_group(&owner, club_id, "Unvollständig").await;
    assert_ok(
        &app.set_group_members(&owner, club_id, incomplete, &c, &[])
            .await,
    );

    let response = app.get("/api/query/startlist", None).await;
    assert_status(&response, StatusCode::OK);
    let acts = response.body.as_array().unwrap();
    assert_eq!(acts.len(), 1);
    let act = &acts[0];
    assert_eq!(act["id"], group.to_string());
    assert_eq!(act["group_name"], "Wind;Wetter");
    assert_eq!(act["name"], "=Sturmfahrt");
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(act["category"], "KGU15");
    let participants = act["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 4);
    let replacement = &participants[3];
    assert_eq!(replacement["id"], b[1].to_string());
    assert_eq!(replacement["is_replacement"], true);
    assert_eq!(replacement["club_name"], "SSV");
    // The public startlist leaves out club ids and ages.
    assert!(replacement.get("club_id").is_none(), "{replacement}");
    assert!(act.get("max_age").is_none(), "{act}");

    let response = app.get("/api/query/get_startlist_csv", None).await;
    assert_eq!(response.status, StatusCode::OK);
    let csv = response.body.as_str().unwrap();
    let rows: Vec<_> = csv
        .lines()
        .filter(|line| line.starts_with("Kleingruppe Junior Expert (U15);"))
        .collect();
    assert_eq!(rows.len(), 1, "{csv}");
    let fields: Vec<_> = rows[0].split(';').collect();
    assert_eq!(fields.len(), 8, "{fields:?}");
    // The rider count leaves out the replacement; clubs are listed once each.
    assert_eq!(
        fields[..7],
        [
            "Kleingruppe Junior Expert (U15)",
            "3",
            "1",
            "Deutschland",
            "'=Sturmfahrt",
            "Wind,Wetter",
            "RSV & SSV",
        ]
    );
    assert!(!csv.contains("Unvollständig"), "{csv}");
    assert!(!csv.contains("A1 Muster"), "{csv}");
}

#[sqlx::test]
async fn group_commands_need_open_registration_unless_admin(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let admin = app.create_admin().await;
    let r = app.add_riders(&owner, club_id, "R", 4).await;
    let group = app.create_group(&owner, club_id, "Wirbelwind").await;
    assert_ok(
        &app.set_group_members(&owner, club_id, group, &r[..3], &[])
            .await,
    );
    app.close_registration().await;

    let commands = [
        (
            "/api/command/create_club_group",
            json!({ "act_type": "group_small", "club_id": club_id, "group_name": "Nachzügler" }),
        ),
        (
            "/api/command/set_club_group_members",
            json!({
                "club_id": club_id,
                "act_id": group,
                "members": [{ "rider_id": r[3], "is_replacement": true }],
            }),
        ),
        (
            "/api/command/rename_club_group",
            json!({ "act_id": group, "group_name": "Umbenannt" }),
        ),
        (
            "/api/command/set_club_group_type",
            json!({ "act_id": group, "act_type": "group_large" }),
        ),
        ("/api/command/delete_club_group", json!({ "act_id": group })),
    ];
    for (uri, body) in &commands {
        let response = app.post(uri, body.clone(), Some(&owner)).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{uri}");
    }
    let act = get_act(&app, group).await;
    assert_eq!(act["group_name"], "Wirbelwind");
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(participant_ids(&act), id_set(&r[..3]));
    assert_eq!(list_groups(&app, &owner).await.len(), 1);

    // Admins can still change everything.
    for (uri, body) in commands {
        let response = app.post(uri, body, Some(&admin)).await;
        assert_ok(&response);
    }
    let groups = list_groups(&app, &admin).await;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["group_name"], "Nachzügler");
}

#[sqlx::test]
async fn incomplete_groups_are_listed_without_category_after_other_acts(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let r = app.add_riders(&cookie, club_id, "R", 3).await;
    let group = app.create_group(&cookie, club_id, "Zu klein").await;
    assert_ok(
        &app.set_group_members(&cookie, club_id, group, &r[..2], &r[2..])
            .await,
    );
    let single = app.add_single_act(&cookie, club_id, "Solo").await;

    let acts = list_acts(&app).await;
    let ids: Vec<_> = acts.iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [single.to_string(), group.to_string()]);
    let act = &acts[1];
    assert_eq!(act["group_name"], "Zu klein");
    assert_eq!(act["act_type"], "group_small");
    assert_eq!(act["category"], Value::Null);
    assert_eq!(act["category_order"], Value::Null);
    assert_eq!(act["participants"].as_array().unwrap().len(), 3);
    assert_eq!(get_act(&app, group).await["category"], Value::Null);

    // It doesn't start, so it isn't in the startlist.
    let response = app.get("/api/query/startlist", None).await;
    let startlist = response.body.as_array().unwrap();
    assert_eq!(startlist.len(), 1);
    assert_eq!(startlist[0]["id"], single.to_string());
}
