//! Integration tests for the start list's approval (#251): until an admin
//! publishes it, the start list and the timeplan are for the organizer team.

mod common;

use axum::http::StatusCode;
use common::{TestApp, rider_body};
use serde_json::{Value, json};

const COMPETITION: &str = include_str!("fixtures/competition.toml");

/// The endpoints with start list or timeplan content.
const HIDDEN_WHILE_UNPUBLISHED: [&str; 4] = [
    "/api/query/startlist",
    "/api/query/get_startlist_csv",
    "/api/query/predict_timeplan",
    "/api/query/list_timeplan",
];

/// An app with an act in a category, whose start list isn't published.
async fn unpublished_app() -> TestApp {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;
    common::set_startlist_published(&app.db, false).await;
    app
}

async fn cookie_with_roles(app: &TestApp, name: &str, roles: &[&str]) -> String {
    let id = app.create_user_with_roles(name, roles).await;
    app.cookie_for(id)
}

async fn set_published(app: &TestApp, cookie: &str, published: bool) -> StatusCode {
    app.post(
        "/api/command/set_startlist_published",
        json!({ "published": published }),
        Some(cookie),
    )
    .await
    .status
}

async fn statuses(app: &TestApp, cookie: Option<&str>) -> Vec<StatusCode> {
    let mut statuses = vec![];
    for uri in HIDDEN_WHILE_UNPUBLISHED {
        statuses.push(app.get(uri, cookie).await.status);
    }
    statuses
}

#[tokio::test]
async fn the_public_gets_nothing_of_an_unpublished_startlist() {
    let app = unpublished_app().await;
    assert_eq!(statuses(&app, None).await, [StatusCode::FORBIDDEN; 4]);
    let (club, _) = app.create_club_owner("Other", "Other RSV").await;
    assert_eq!(
        statuses(&app, Some(&club)).await,
        [StatusCode::FORBIDDEN; 4]
    );
    for role in ["treasurer", "check_in", "music_check"] {
        let cookie = cookie_with_roles(&app, role, &[role]).await;
        assert_eq!(
            statuses(&app, Some(&cookie)).await,
            [StatusCode::FORBIDDEN; 4],
            "{role}"
        );
    }
}

#[tokio::test]
async fn the_organizer_team_sees_the_unpublished_startlist() {
    let app = unpublished_app().await;
    for role in ["admin", "jury_lead", "music_control", "moderation"] {
        let cookie = cookie_with_roles(&app, role, &[role]).await;
        assert_eq!(
            statuses(&app, Some(&cookie)).await,
            [StatusCode::OK; 4],
            "{role}"
        );
        let startlist = app.get("/api/query/startlist", Some(&cookie)).await;
        assert_eq!(startlist.body.as_array().unwrap().len(), 1, "{role}");
    }
}

#[tokio::test]
async fn the_start_order_stays_hidden_in_the_act_list_until_publication() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    let order = |body: &Value| body[0]["act_order"].clone();
    let act = app.get("/api/query/list_acts", Some(&admin)).await.body[0]["id"].clone();
    let response = app
        .post(
            "/api/command/set_act_order",
            json!({ "act_id": act, "order": 1 }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let public = app.get("/api/query/list_acts", None).await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(order(&public.body), Value::Null);
    let staff = app.get("/api/query/list_acts", Some(&admin)).await;
    assert_ne!(order(&staff.body), Value::Null);

    assert_eq!(set_published(&app, &admin, true).await, StatusCode::OK);
    let public = app.get("/api/query/list_acts", None).await;
    assert_ne!(order(&public.body), Value::Null);
}

#[tokio::test]
async fn only_admins_publish_and_revoke() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    for role in ["jury_lead", "music_control", "moderation", "treasurer"] {
        let cookie = cookie_with_roles(&app, role, &[role]).await;
        assert_eq!(
            set_published(&app, &cookie, true).await,
            StatusCode::FORBIDDEN,
            "{role}"
        );
    }
    let response = app
        .post(
            "/api/command/set_startlist_published",
            json!({ "published": true }),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(statuses(&app, None).await, [StatusCode::FORBIDDEN; 4]);

    assert_eq!(set_published(&app, &admin, true).await, StatusCode::OK);
    let jury_lead = cookie_with_roles(&app, "Lead", &["jury_lead"]).await;
    assert_eq!(
        set_published(&app, &jury_lead, false).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(statuses(&app, None).await, [StatusCode::OK; 4]);
}

#[tokio::test]
async fn publishing_opens_it_to_everyone_and_changes_need_no_new_approval() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    assert_eq!(set_published(&app, &admin, true).await, StatusCode::OK);
    assert_eq!(statuses(&app, None).await, [StatusCode::OK; 4]);

    // A rider registering later shows up right away.
    let (owner, club_id) = app.create_club_owner("Second", "Second RSV").await;
    app.add_rider(&owner, rider_body(club_id, "Berta", true, false))
        .await;
    let startlist = app.get("/api/query/startlist", None).await;
    assert_eq!(startlist.body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn revoking_returns_it_to_the_organizer_team() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    assert_eq!(set_published(&app, &admin, true).await, StatusCode::OK);
    assert_eq!(set_published(&app, &admin, false).await, StatusCode::OK);
    assert_eq!(statuses(&app, None).await, [StatusCode::FORBIDDEN; 4]);
    assert_eq!(statuses(&app, Some(&admin)).await, [StatusCode::OK; 4]);
}

#[tokio::test]
async fn the_system_status_tells_whether_it_is_published() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    let published = async |app: &TestApp| {
        app.get("/api/query/get_system_status", None).await.body["startlist_published"].clone()
    };
    assert_eq!(published(&app).await, json!(false));
    set_published(&app, &admin, true).await;
    assert_eq!(published(&app).await, json!(true));
}

#[tokio::test]
async fn every_new_competition_starts_unpublished() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    let admin = app.create_admin().await;
    let import = async |file: &str| {
        app.post(
            "/api/command/import_competition",
            json!({ "file": file }),
            Some(&admin),
        )
        .await
    };
    let status = async || app.get("/api/query/startlist", None).await.status;

    assert_eq!(import(COMPETITION).await.status, StatusCode::OK);
    assert_eq!(status().await, StatusCode::FORBIDDEN);
    assert_eq!(set_published(&app, &admin, true).await, StatusCode::OK);
    assert_eq!(status().await, StatusCode::OK);

    // Importing the same competition again keeps its approval.
    assert_eq!(import(COMPETITION).await.status, StatusCode::OK);
    assert_eq!(status().await, StatusCode::OK);

    // A replace starts the competition's planning over and needs a new
    // approval.
    let next = COMPETITION.replacen("fee_per_start = 15", "fee_per_start = 20", 1);
    let response = import(&next).await;
    assert_eq!(response.body["outcome"], "replaced");
    assert_eq!(response.body["dropped"]["startlist_published"], true);
    assert_eq!(status().await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn without_a_competition_nothing_is_published() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    assert_eq!(statuses(&app, None).await, [StatusCode::FORBIDDEN; 4]);
}

#[tokio::test]
async fn a_stale_login_sees_the_published_startlist_like_anyone() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    let stale = Some("jwt=not-a-valid-token");
    // The two routes that never looked at the login before.
    let both = async |app: &TestApp| {
        vec![
            app.get("/api/query/startlist", stale).await.status,
            app.get("/api/query/get_startlist_csv", stale).await.status,
        ]
    };
    assert_eq!(both(&app).await, [StatusCode::FORBIDDEN; 2]);
    set_published(&app, &admin, true).await;
    assert_eq!(both(&app).await, [StatusCode::OK; 2]);
}

#[tokio::test]
async fn get_act_hides_the_start_order_until_publication() {
    let app = unpublished_app().await;
    let admin = app.create_admin().await;
    let acts = app.get("/api/query/list_acts", Some(&admin)).await.body;
    let act = acts[0]["id"].as_str().unwrap().to_string();
    app.post(
        "/api/command/set_act_order",
        json!({ "act_id": act, "order": 1 }),
        Some(&admin),
    )
    .await;
    let uri = format!("/api/query/get_act?act_id={act}");
    assert_eq!(app.get(&uri, None).await.body["act_order"], Value::Null);
    assert_eq!(app.get(&uri, Some(&admin)).await.body["act_order"], 1);
    set_published(&app, &admin, true).await;
    assert_eq!(app.get(&uri, None).await.body["act_order"], 1);
}
