//! Integration tests for admin-only endpoints, categories and the timeplan.

mod common;

use axum::http::StatusCode;
use common::{RELOAD_DB_TOKEN, TestApp};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Iso8601};

async fn category_orders(app: &TestApp) -> Vec<(String, Option<i64>)> {
    let response = app.get("/api/query/list_categories", None).await;
    assert_eq!(response.status, StatusCode::OK);
    response
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].as_str().unwrap().to_string(), c["order"].as_i64()))
        .collect()
}

fn order_of(categories: &[(String, Option<i64>)], name: &str) -> Option<i64> {
    categories.iter().find(|(n, _)| n == name).unwrap().1
}

fn category_body(name: &str) -> Value {
    json!({
        "name": name,
        "description": "Testkategorie",
        // Format sent by the admin frontend.
        "from_birthday": "2000-01-01T00:00:00Z",
        "to_birthday": "2010-01-01T00:00:00Z",
        "act_type": "single",
        "is_single_male": false,
        "einfahrzeit_seconds": 300,
        "act_duration_seconds": 120,
        "judge_duration_seconds": 60,
    })
}

#[tokio::test]
async fn list_users_is_admin_only() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    let response = app.get("/api/query/list_users", Some(&admin)).await;
    assert_eq!(response.status, StatusCode::OK);
    let mut names: Vec<_> = response
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["Admin", "Alex"]);

    let forbidden = app.get("/api/query/list_users", Some(&user)).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);

    let anonymous = app.get("/api/query/list_users", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admins_can_grant_and_revoke_admin() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user_id = app.create_user("Alex", false).await;

    let promoted = app
        .post(
            "/api/command/set_user_roles",
            json!({ "user_id": user_id, "roles": ["admin"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(promoted.status, StatusCode::OK, "{:?}", promoted.body);
    let users = app.get("/api/query/list_users", Some(&admin)).await;
    assert_eq!(
        users
            .body
            .as_array()
            .unwrap()
            .iter()
            .find(|user| user["id"] == user_id.to_string())
            .unwrap()["roles"],
        json!(["admin"])
    );

    let demoted = app
        .post(
            "/api/command/set_user_roles",
            json!({ "user_id": user_id, "roles": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(demoted.status, StatusCode::OK, "{:?}", demoted.body);
    let users = app.get("/api/query/list_users", Some(&admin)).await;
    assert_eq!(
        users
            .body
            .as_array()
            .unwrap()
            .iter()
            .find(|user| user["id"] == user_id.to_string())
            .unwrap()["roles"],
        json!([])
    );
}

#[tokio::test]
async fn setting_roles_is_admin_only_and_checks_the_user() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user_id = app.create_user("Alex", false).await;
    let user = app.cookie_for(user_id);
    let body = json!({ "user_id": user_id, "roles": ["admin"] });

    assert_eq!(
        app.post("/api/command/set_user_roles", body.clone(), Some(&user))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.post("/api/command/set_user_roles", body, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    let missing = app
        .post(
            "/api/command/set_user_roles",
            json!({ "user_id": uuid::Uuid::now_v7(), "roles": ["admin"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_lone_admin_keeps_admin_as_nobody_drops_their_own() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin_id = app.create_user("Admin", true).await;
    let admin = app.cookie_for(admin_id);

    let response = app
        .post(
            "/api/command/set_user_roles",
            json!({ "user_id": admin_id, "roles": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);

    let is_admin: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_roles WHERE user_id = ? AND role = 'admin')",
    )
    .bind(admin_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(is_admin);
}

#[tokio::test]
async fn admin_cannot_demote_themselves_even_when_another_admin_exists() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin_id = app.create_user("Admin", true).await;
    app.create_user("Another admin", true).await;
    let admin = app.cookie_for(admin_id);

    let response = app
        .post(
            "/api/command/set_user_roles",
            json!({ "user_id": admin_id, "roles": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);

    let is_admin: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_roles WHERE user_id = ? AND role = 'admin')",
    )
    .bind(admin_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(is_admin);
}

#[tokio::test]
async fn list_riders_is_admin_only() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    assert_eq!(
        app.get("/api/query/list_riders", Some(&admin)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.get("/api/query/list_riders", Some(&user)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get("/api/query/list_riders", None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn system_status_reflects_open_registration() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let response = app.get("/api/query/get_system_status", None).await;
    assert_eq!(response.status, StatusCode::OK);
    let mut body = response.body;
    let competition = body.as_object_mut().unwrap().remove("competition").unwrap();
    assert_eq!(
        body,
        json!({
            "can_register": true,
            "can_create_club": true,
            "can_register_rider": true,
            "can_upload_music": true,
            "can_register_judge": true,
            "jury_registration": true,
            "moderation_input": true,
            "startlist_published": true,
        })
    );
    assert_eq!(competition["name"], "Einradevent 2027 (Demo)");
    assert_eq!(competition["determining_day"], "2027-06-01");
    assert_eq!(competition["fee_per_start"], 15.0);
    assert_eq!(competition["max_fee_per_rider"], 45.0);
    assert_eq!(competition["payment_reference"], "Startgeld");
}

#[tokio::test]
async fn everything_is_closed_without_competition() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    sqlx::query("DELETE FROM competition")
        .execute(&app.db)
        .await
        .unwrap();

    let response = app.get("/api/query/get_system_status", None).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["competition"], Value::Null);
    assert_eq!(response.body["can_register"], false);
    assert_eq!(response.body["can_register_rider"], false);
    assert_eq!(response.body["can_upload_music"], false);
}

#[tokio::test]
async fn categories_are_seeded_in_order() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let categories = category_orders(&app).await;

    assert_eq!(categories.len(), 40);
    assert_eq!(categories[0], ("EMU11".to_string(), Some(1)));
    let orders: Vec<_> = categories.iter().map(|(_, o)| o.unwrap()).collect();
    assert!(orders.is_sorted());
}

#[tokio::test]
async fn category_commands_require_admin() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    let as_user = app
        .post("/api/command/add_category", category_body("X"), Some(&user))
        .await;
    assert_eq!(as_user.status, StatusCode::FORBIDDEN);

    let anonymous = app
        .post("/api/command/add_category", category_body("X"), None)
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    for uri in [
        "/api/command/delete_category",
        "/api/command/move_category_up",
        "/api/command/move_category_down",
    ] {
        let response = app.post(uri, json!({ "name": "EMU11" }), Some(&user)).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{uri}");
    }
    assert_eq!(category_orders(&app).await.len(), 40);
}

#[tokio::test]
async fn admin_can_add_and_edit_category() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let added = app
        .post(
            "/api/command/add_category",
            category_body("TEST"),
            Some(&admin),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);

    let mut edit = category_body("TEST");
    edit["new_name"] = json!("RENAMED");
    edit["act_duration_seconds"] = json!(999);
    let edited = app
        .post("/api/command/edit_category", edit, Some(&admin))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);

    let response = app.get("/api/query/list_categories", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let categories = response.body.as_array().unwrap();
    assert!(!categories.iter().any(|c| c["name"] == "TEST"));
    let renamed = categories.iter().find(|c| c["name"] == "RENAMED").unwrap();
    assert_eq!(renamed["act_duration_seconds"], 999);
    assert_eq!(renamed["description"], "Testkategorie");
    // New categories go to the end of the order.
    let last = categories
        .iter()
        .map(|c| c["order"].as_i64().unwrap())
        .max()
        .unwrap();
    assert_eq!(renamed["order"], last);
}

#[tokio::test]
#[ignore = "known bug: category birthdays are stored unvalidated and break list_categories"]
async fn category_with_date_only_birthday_does_not_break_listing() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let mut body = category_body("DATE-ONLY");
    body["from_birthday"] = json!("2000-01-01");

    let added = app
        .post("/api/command/add_category", body, Some(&admin))
        .await;

    // Either the input is rejected, or the listing keeps working.
    if added.status == StatusCode::OK {
        let response = app.get("/api/query/list_categories", None).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
}

#[tokio::test]
async fn move_category_up_and_down_swaps_neighbours() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let up = app
        .post(
            "/api/command/move_category_up",
            json!({ "name": "EMU13" }),
            Some(&admin),
        )
        .await;
    assert_eq!(up.status, StatusCode::OK, "{:?}", up.body);
    let categories = category_orders(&app).await;
    assert_eq!(order_of(&categories, "EMU13"), Some(1));
    assert_eq!(order_of(&categories, "EMU11"), Some(2));

    let down = app
        .post(
            "/api/command/move_category_down",
            json!({ "name": "EMU13" }),
            Some(&admin),
        )
        .await;
    assert_eq!(down.status, StatusCode::OK, "{:?}", down.body);
    let categories = category_orders(&app).await;
    assert_eq!(order_of(&categories, "EMU11"), Some(1));
    assert_eq!(order_of(&categories, "EMU13"), Some(2));
}

#[tokio::test]
async fn moving_first_category_up_or_last_down_is_rejected() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let before = category_orders(&app).await;

    let up = app
        .post(
            "/api/command/move_category_up",
            json!({ "name": "EMU11" }),
            Some(&admin),
        )
        .await;
    assert_eq!(up.status, StatusCode::BAD_REQUEST);

    let down = app
        .post(
            "/api/command/move_category_down",
            json!({ "name": "GG15+" }),
            Some(&admin),
        )
        .await;
    assert_eq!(down.status, StatusCode::BAD_REQUEST);

    assert_eq!(category_orders(&app).await, before);
}

#[tokio::test]
async fn delete_category_removes_its_timeplan_entries() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let response = app
        .post(
            "/api/command/delete_category",
            json!({ "name": "EWU13" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);

    let categories = category_orders(&app).await;
    assert!(!categories.iter().any(|(name, _)| name == "EWU13"));
    let timeplan_refs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM timeplan WHERE category = 'EWU13'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(timeplan_refs, 0);
}

#[tokio::test]
async fn delete_unknown_category_is_not_found() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/delete_category",
            json!({ "name": "DOES-NOT-EXIST" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

fn parse_time(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Iso8601::DEFAULT).unwrap()
}

#[tokio::test]
async fn predicted_timeplan_is_contiguous_and_planned() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let timeplan_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeplan")
        .fetch_one(&app.db)
        .await
        .unwrap();

    let response = app.get("/api/query/predict_timeplan", None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let items = response.body["items"].as_array().unwrap();
    assert_eq!(items.len() as i64, timeplan_rows);

    let mut previous_end: Option<OffsetDateTime> = None;
    for item in items {
        assert_eq!(item["status"], "Planned");
        let start = parse_time(&item["planned_start"]);
        let end = parse_time(&item["planned_end"]);
        assert_eq!(
            (end - start).whole_seconds(),
            item["planned_duration"].as_i64().unwrap()
        );
        if let Some(previous_end) = previous_end {
            assert!(start >= previous_end, "{item}");
        }
        previous_end = Some(end);
    }

    // A category without acts is skipped: it takes no time, not even for the
    // warm-up, but still reports its configured warm-up time.
    let first = &items[0]["timeplan_entry"]["Acts"];
    assert_eq!(first["home_category"], "EMU11");
    assert_eq!(first["manually_ordered"], false);
    assert_eq!(first["duration_seconds"], 0);
    assert!(first["einfahrzeit_seconds"].as_i64().unwrap() > 0);
    assert!(first["slots"].as_array().unwrap().is_empty());
    assert_eq!(items[0]["planned_start"], items[1]["planned_start"]);

    let lunch = items
        .iter()
        .find(|i| i["timeplan_entry"]["Custom"]["label"] == "Mittagspause")
        .unwrap();
    assert_eq!(lunch["planned_duration"], 45 * 60);
}

#[tokio::test]
async fn timeplan_forward_starts_first_entry() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    let forbidden = app
        .post("/api/command/timeplan_forward", json!(null), Some(&user))
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);

    let response = app
        .post("/api/command/timeplan_forward", json!(null), Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // Without acts, every start block is empty: they are skipped up to
    // the first custom entry, which starts.
    let timeplan = app.get("/api/query/predict_timeplan", None).await;
    let items = timeplan.body["items"].as_array().unwrap();
    let started = items
        .iter()
        .position(|i| i["status"] == "Started")
        .expect("one entry started");
    assert!(items[started]["timeplan_entry"]["Custom"].is_object());
    assert!(items[..started].iter().all(|i| i["status"] == "Ended"));
    assert!(
        items[started + 1..]
            .iter()
            .all(|i| i["status"] == "Planned")
    );
}

#[tokio::test]
async fn reload_db_rejects_non_admin_without_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    let response = app
        .post(
            "/api/command/reload_db?token=wrong",
            json!(null),
            Some(&user),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);

    // The token alone is not enough: the endpoint also requires a login.
    let anonymous = app
        .post(
            &format!("/api/command/reload_db?token={RELOAD_DB_TOKEN}"),
            json!(null),
            None,
        )
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn reload_db_token_path_is_off_without_a_configured_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_reload_token(db).await;
    let user = app.cookie_for(app.create_user("Alex", false).await);

    for token in ["", "wrong", "reload_db", RELOAD_DB_TOKEN] {
        let response = app
            .post(
                &format!("/api/command/reload_db?token={token}"),
                json!(null),
                Some(&user),
            )
            .await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "token {token:?}");
    }
    let no_token = app
        .post("/api/command/reload_db", json!(null), Some(&user))
        .await;
    assert_eq!(no_token.status, StatusCode::FORBIDDEN);

    let admin = app.cookie_for(app.create_user("Max Müller", true).await);
    let response = app
        .post("/api/command/reload_db", json!(null), Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_api_route_is_not_found() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let response = app.get("/api/query/does_not_exist", None).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn swagger_serves_openapi_document() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let response = app.get("/openapi.json", None).await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body["paths"]["/api/command/register"].is_object());
}
