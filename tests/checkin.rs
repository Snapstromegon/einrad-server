//! Integration tests for the admin tools of the competition day: the check-in
//! of riders and the club overview.

mod common;

use std::collections::HashMap;

use axum::http::StatusCode;
use common::{TestApp, TestResponse, rider_body};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

/// A club with Anna (single), Berta and Carla (pair). Returns the owner's
/// cookie, the club id and the rider ids.
async fn club_with_riders(app: &TestApp) -> (String, Uuid, [Uuid; 3]) {
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app
        .add_rider(&cookie, rider_body(club_id, "Anna", true, false))
        .await;
    let berta = app
        .add_rider(&cookie, rider_body(club_id, "Berta", false, true))
        .await;
    let mut carla = rider_body(club_id, "Carla", false, true);
    carla["partner_id"] = json!(berta);
    let carla = app.add_rider(&cookie, carla).await;
    (cookie, club_id, [anna, berta, carla])
}

async fn set_checked_in(
    app: &TestApp,
    cookie: Option<&str>,
    rider_ids: &[Uuid],
    checked_in: bool,
) -> TestResponse {
    app.post(
        "/api/command/set_checked_in",
        json!({ "rider_ids": rider_ids, "checked_in": checked_in }),
        cookie,
    )
    .await
}

/// The check-in time of every rider (`null` if not checked in), by id.
async fn check_ins(app: &TestApp, admin: &str) -> HashMap<Uuid, Value> {
    let response = app.get("/api/query/list_check_ins", Some(admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["rider_id"].as_str().unwrap().parse().unwrap(),
                c["checked_in_at"].clone(),
            )
        })
        .collect()
}

/// Ids of the riders that are checked in.
async fn checked_in(app: &TestApp, admin: &str) -> Vec<Uuid> {
    let mut ids: Vec<_> = check_ins(app, admin)
        .await
        .into_iter()
        .filter(|(_, at)| !at.is_null())
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    ids
}

fn sorted(ids: &[Uuid]) -> Vec<Uuid> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids
}

#[sqlx::test]
async fn check_in_is_admin_only(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, _, [anna, ..]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;

    let response = set_checked_in(&app, Some(&owner), &[anna], true).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        response.body,
        json!({ "type": "StatusCode", "message": 403 })
    );
    let response = set_checked_in(&app, None, &[anna], true).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    let response = app.get("/api/query/list_check_ins", Some(&owner)).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    let response = app.get("/api/query/list_check_ins", None).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    assert!(checked_in(&app, &admin).await.is_empty());
}

#[sqlx::test]
async fn riders_are_checked_in_and_out_one_by_one_and_in_bulk(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (_, _, [anna, berta, carla]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;

    // Every rider is listed, none checked in yet.
    let all = check_ins(&app, &admin).await;
    assert_eq!(all.len(), 3);
    assert!(all.values().all(Value::is_null), "{all:?}");

    let response = set_checked_in(&app, Some(&admin), &[anna], true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let checked_in_at = &response.body["checked_in_at"];
    assert!(checked_in_at.is_string(), "{:?}", response.body);
    assert_eq!(check_ins(&app, &admin).await[&anna], *checked_in_at);

    let response = set_checked_in(&app, Some(&admin), &[berta, carla], true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        checked_in(&app, &admin).await,
        sorted(&[anna, berta, carla])
    );

    let response = set_checked_in(&app, Some(&admin), &[anna, berta], false).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["checked_in_at"], Value::Null);
    assert_eq!(checked_in(&app, &admin).await, [carla]);

    let response = set_checked_in(&app, Some(&admin), &[carla], false).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(checked_in(&app, &admin).await.is_empty());
}

#[sqlx::test]
async fn checking_in_again_keeps_the_first_time(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (_, _, [anna, berta, carla]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;

    set_checked_in(&app, Some(&admin), &[anna], true).await;
    let first = check_ins(&app, &admin).await[&anna].clone();

    let response = set_checked_in(&app, Some(&admin), &[anna, berta], true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let second = &response.body["checked_in_at"];
    assert_ne!(*second, first);
    let all = check_ins(&app, &admin).await;
    assert_eq!(all[&anna], first);
    assert_eq!(all[&berta], *second);

    // Listing a rider twice does no harm.
    let response = set_checked_in(&app, Some(&admin), &[carla, carla], true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        checked_in(&app, &admin).await,
        sorted(&[anna, berta, carla])
    );
}

#[sqlx::test]
async fn unknown_riders_fail_the_whole_call(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (_, _, [anna, berta, _]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;
    let unknown = Uuid::now_v7();

    let response = set_checked_in(&app, Some(&admin), &[anna, unknown, berta], true).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(
        response.body,
        json!({ "type": "StatusCode", "message": 404 })
    );
    assert!(checked_in(&app, &admin).await.is_empty());

    set_checked_in(&app, Some(&admin), &[anna, berta], true).await;
    let before = check_ins(&app, &admin).await;
    let response = set_checked_in(&app, Some(&admin), &[anna, unknown, berta], false).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(check_ins(&app, &admin).await, before);
}

#[sqlx::test]
async fn list_riders_shows_the_check_in(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (_, _, [anna, berta, carla]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;
    set_checked_in(&app, Some(&admin), &[anna], true).await;
    let anna_at = check_ins(&app, &admin).await[&anna].clone();

    let response = app.get("/api/query/list_riders", Some(&admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let riders = response.body.as_array().unwrap();
    let checked_in_at = |id: Uuid| {
        riders.iter().find(|s| s["id"] == id.to_string()).unwrap()["checked_in_at"].clone()
    };
    assert_eq!(checked_in_at(anna), anna_at);
    assert_eq!(checked_in_at(berta), Value::Null);
    assert_eq!(checked_in_at(carla), Value::Null);
}

#[sqlx::test]
async fn check_in_works_after_registration_closed(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id, [anna, ..]) = club_with_riders(&app).await;
    let admin = app.create_admin().await;
    app.close_registration().await;

    // Clubs can't change anything any more …
    let response = app
        .post(
            "/api/command/add_rider",
            rider_body(club_id, "Dora", true, false),
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    // … but the check-in runs on competition day.
    let response = set_checked_in(&app, Some(&admin), &[anna], true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(checked_in(&app, &admin).await, [anna]);
}

#[sqlx::test]
async fn public_endpoints_do_not_expose_the_check_in(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id, riders) = club_with_riders(&app).await;
    let admin = app.create_admin().await;
    let group_riders = app.add_riders(&owner, club_id, "R", 3).await;
    let group = app.create_group(&owner, club_id, "Wirbelwind").await;
    let response = app
        .set_group_members(&owner, club_id, group, &group_riders, &[])
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let all: Vec<Uuid> = riders.iter().chain(&group_riders).copied().collect();
    let response = set_checked_in(&app, Some(&admin), &all, true).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let single = app.act_of(riders[0]).await;

    let public = [
        "/api/query/list_acts".to_string(),
        "/api/query/startlist".to_string(),
        "/api/query/get_startlist_csv".to_string(),
        format!("/api/query/get_act?act_id={single}"),
        format!("/api/query/get_act?act_id={group}"),
    ];
    let club = [
        "/api/query/list_groups".to_string(),
        format!("/api/query/list_club_groups?club_id={club_id}"),
        format!("/api/query/list_club_acts?club_id={club_id}"),
        format!("/api/query/list_club_riders?club_id={club_id}"),
    ];
    let requests = public
        .iter()
        .map(|uri| (uri, None))
        .chain(club.iter().map(|uri| (uri, Some(owner.as_str()))));
    for (uri, cookie) in requests {
        let response = app.get(uri, cookie).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{uri}: {:?}",
            response.body
        );
        let body = response.body.to_string();
        assert!(!body.contains("checked_in"), "{uri}: {body}");
    }
}

#[sqlx::test]
async fn list_clubs_shows_clubs_with_owners_to_admins(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (_, other_club_id) = app.create_club_owner("Other", "ESV").await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/create_booking",
            json!({ "club_id": club_id, "amount": 42.5, "date": "2027-03-01" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // Same as list_users: no access without a role.
    let response = app.get("/api/query/list_clubs", Some(&owner)).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    let response = app.get("/api/query/list_clubs", None).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    let response = app.get("/api/query/list_clubs", Some(&admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let clubs = response.body.as_array().unwrap();
    // Sorted by name.
    assert_eq!(clubs.len(), 2);
    assert_eq!(clubs[0]["id"], other_club_id.to_string());
    assert_eq!(clubs[0]["paid"], 0.0);
    let rsv = &clubs[1];
    let owner_id: Uuid = rsv["owner_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        *rsv,
        json!({
            "id": club_id,
            "name": "RSV",
            "paid": 42.5,
            "owner_id": owner_id,
            "contacts": {
                "contact_name": "Owner",
                "contact_email": "owner@cup.test",
                "contact_email_verified": true,
                "representative_name": "Owner",
                "representative_email": "owner@cup.test",
            },
            "code": nrw_freestyle_cup_registration::utils::club_code(&club_id),
            "amount_due": { "start_fees": 0.0, "jury_fee": 0.0, "total": 0.0, "waived": 0.0 },
            "fee_waiver": null,
            "jury": {
                "acts": 0,
                "required": 0,
                "registered": 0,
                "credited_override": null,
                "credited": 0,
                "missing": 0,
                "fee": 0.0,
            },
        })
    );
    let whoami = app.get("/api/query/whoami", Some(&owner)).await;
    assert_eq!(whoami.body["id"], owner_id.to_string());
}
