//! Integration tests for "Wettkampf bearbeiten": editing the competition in
//! the admin UI (ADR 0005).

mod common;

use axum::http::StatusCode;
use common::{TempDir, TestApp, TestResponse, rider_body};
use serde_json::{Value, json};
use time::{OffsetDateTime, macros::datetime};

async fn settings(app: &TestApp, cookie: &str) -> Value {
    let response = app
        .get("/api/query/get_competition_settings", Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

async fn edit(app: &TestApp, cookie: &str, competition: &Value, photos: &Value) -> TestResponse {
    app.post(
        "/api/command/edit_competition",
        json!({ "competition": competition, "photos": photos }),
        Some(cookie),
    )
    .await
}

fn message(response: &TestResponse) -> &str {
    response.body["message"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn only_admins_see_and_edit_the_competition() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let competition = settings(&app, &admin).await["competition"].clone();
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let cookie = app.cookie_for(user);
        assert_eq!(
            app.get("/api/query/get_competition_settings", Some(&cookie))
                .await
                .status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            edit(&app, &cookie, &competition, &Value::Null).await.status,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn the_settings_show_the_photo_password_and_the_feature_defaults() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    common::set_photos(
        &app.db,
        Some(&einrad_server::photos::PhotosSettings {
            source: "https://cloud.example.org/s/AbC123".into(),
            user: None,
            password: Some("geheim".into()),
        }),
    )
    .await;
    let admin = app.create_admin().await;
    let body = settings(&app, &admin).await;
    assert_eq!(body["photos"]["password"], "geheim");
    assert_eq!(
        body["feature_defaults"],
        json!({ "jury_registration": true, "moderation_input": true })
    );
    assert_eq!(
        body["competition"]["features"],
        json!({ "jury_registration": null, "moderation_input": null })
    );
    assert_eq!(body["file_id_mismatch"], Value::Null);
}

#[tokio::test]
async fn edits_apply_right_away() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;
    let club = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(club.body["amount_due"]["total"], 15.0);

    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["fee_per_start"] = json!(20);
    competition["venue"] = json!("  ");
    competition["features"] = json!({ "jury_registration": true, "moderation_input": false });
    let photos = TempDir::new();
    let response = edit(
        &app,
        &admin,
        &competition,
        &json!({ "source": photos.0.display().to_string(), "user": "", "password": "" }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["warnings"], json!([]));

    let club = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(club.body["amount_due"]["start_fees"], 20.0);
    let status = app.get("/api/query/get_system_status", None).await.body;
    assert_eq!(status["jury_registration"], true);
    assert_eq!(status["moderation_input"], false);
    assert_eq!(status["competition"]["venue"], Value::Null);
    assert!(app.photos.configured());
    let body = settings(&app, &admin).await;
    assert_eq!(body["photos"]["user"], Value::Null);
    assert_eq!(body["photos"]["password"], Value::Null);

    // No source, no photos.
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(!app.photos.configured());
}

#[tokio::test]
async fn invalid_values_are_rejected_like_in_the_file() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let competition = settings(&app, &admin).await["competition"].clone();
    let before = competition.clone();

    let mut late = competition.clone();
    late["registration_end"] = json!("2030-01-01T00:00:00+01:00");
    let response = edit(&app, &admin, &late, &Value::Null).await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        message(&response).contains("registration_end must not be after start_date"),
        "{:?}",
        response.body
    );

    let mut nameless = competition.clone();
    nameless["name"] = json!(" ");
    let response = edit(&app, &admin, &nameless, &Value::Null).await;
    assert!(
        message(&response).contains("name must not be empty"),
        "{:?}",
        response.body
    );

    let response = edit(
        &app,
        &admin,
        &competition,
        &json!({ "source": "https://cloud.example.org/s/", "user": null, "password": null }),
    )
    .await;
    assert!(
        message(&response).contains("[competition.photos]"),
        "{:?}",
        response.body
    );

    assert_eq!(settings(&app, &admin).await["competition"], before);
}

#[tokio::test]
async fn an_invalid_iban_saves_with_a_warning() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["payment_iban"] = json!("DE00");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body["warnings"][0]
            .as_str()
            .unwrap()
            .contains("IBAN"),
        "{:?}",
        response.body
    );
}

/// Replaces all categories with one per `(name, from, to)`.
async fn set_categories(
    app: &TestApp,
    categories: &[(&str, Option<OffsetDateTime>, Option<OffsetDateTime>)],
) {
    for sql in [
        "DELETE FROM timeplan",
        "DELETE FROM final_categories",
        "DELETE FROM categories",
    ] {
        sqlx::query(sql).execute(&app.db).await.unwrap();
    }
    for (name, from, to) in categories {
        sqlx::query(
            "INSERT INTO categories (name, from_birthday, to_birthday, act_type) \
             VALUES (?, ?, ?, 'single')",
        )
        .bind(name)
        .bind(from)
        .bind(to)
        .execute(&app.db)
        .await
        .unwrap();
    }
}

async fn bounds(app: &TestApp, name: &str) -> (Option<OffsetDateTime>, Option<OffsetDateTime>) {
    sqlx::query_as("SELECT from_birthday, to_birthday FROM categories WHERE name = ?")
        .bind(name)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn set_determining_day(app: &TestApp, day: &str) {
    sqlx::query("UPDATE competition SET determining_day = ?")
        .bind(day)
        .execute(&app.db)
        .await
        .unwrap();
}

/// Edits the determining day to `day` and expects success.
async fn edit_determining_day(app: &TestApp, cookie: &str, day: &str) {
    let mut competition = settings(app, cookie).await["competition"].clone();
    competition["determining_day"] = json!(day);
    let response = edit(app, cookie, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        settings(app, cookie).await["competition"]["determining_day"],
        day
    );
}

#[tokio::test]
async fn editing_the_determining_day_moves_the_matching_category_bounds() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    set_determining_day(&app, "2027-06-01").await;
    set_categories(
        &app,
        &[
            (
                "Matching",
                Some(datetime!(2012-06-01 0:00 UTC)),
                Some(datetime!(2017-06-01 0:00 UTC)),
            ),
            (
                "Other day",
                Some(datetime!(2012-06-02 0:00 UTC)),
                Some(datetime!(2017-07-01 0:00 UTC)),
            ),
            (
                "Not midnight",
                Some(datetime!(2012-06-01 10:00 UTC)),
                Some(datetime!(2017-06-01 0:00:01 UTC)),
            ),
            (
                "Half",
                Some(datetime!(2012-06-01 0:00 UTC)),
                Some(datetime!(2017-12-01 0:00 UTC)),
            ),
        ],
    )
    .await;

    edit_determining_day(&app, &admin, "2027-07-15").await;

    assert_eq!(
        bounds(&app, "Matching").await,
        (
            Some(datetime!(2012-07-15 0:00 UTC)),
            Some(datetime!(2017-07-15 0:00 UTC))
        )
    );
    assert_eq!(
        bounds(&app, "Other day").await,
        (
            Some(datetime!(2012-06-02 0:00 UTC)),
            Some(datetime!(2017-07-01 0:00 UTC))
        )
    );
    assert_eq!(
        bounds(&app, "Not midnight").await,
        (
            Some(datetime!(2012-06-01 10:00 UTC)),
            Some(datetime!(2017-06-01 0:00:01 UTC))
        )
    );
    // Each bound is judged on its own.
    assert_eq!(
        bounds(&app, "Half").await,
        (
            Some(datetime!(2012-07-15 0:00 UTC)),
            Some(datetime!(2017-12-01 0:00 UTC))
        )
    );
}

#[tokio::test]
async fn saving_with_the_same_determining_day_leaves_the_categories_alone() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    set_determining_day(&app, "2027-06-01").await;
    set_categories(
        &app,
        &[("Matching", Some(datetime!(2012-06-01 0:00 UTC)), None)],
    )
    .await;
    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["name"] = json!("Anderer Name");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        bounds(&app, "Matching").await,
        (Some(datetime!(2012-06-01 0:00 UTC)), None)
    );
}

#[tokio::test]
async fn open_bounds_stay_even_when_the_determining_day_looks_like_them() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    set_determining_day(&app, "2027-01-01").await;
    set_categories(
        &app,
        &[
            (
                "Young",
                Some(datetime!(2012-01-01 0:00 UTC)),
                Some(datetime!(9999-12-31 0:00 UTC)),
            ),
            (
                "Old",
                Some(datetime!(1900-01-01 0:00 UTC)),
                Some(datetime!(2012-01-01 0:00 UTC)),
            ),
        ],
    )
    .await;
    edit_determining_day(&app, &admin, "2027-12-31").await;
    assert_eq!(
        bounds(&app, "Young").await,
        (
            Some(datetime!(2012-12-31 0:00 UTC)),
            Some(datetime!(9999-12-31 0:00 UTC))
        )
    );
    assert_eq!(
        bounds(&app, "Old").await,
        (
            Some(datetime!(1900-01-01 0:00 UTC)),
            Some(datetime!(2012-12-31 0:00 UTC))
        )
    );
}

#[tokio::test]
async fn a_leap_day_only_matches_itself_and_clamps_in_other_years() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    set_determining_day(&app, "2028-02-29").await;
    set_categories(
        &app,
        &[
            // 29 Feb matches; 28 Feb (what a one-year bound looks like in a
            // non-leap year) is a different day and stays.
            (
                "Leap",
                Some(datetime!(2016-02-29 0:00 UTC)),
                Some(datetime!(2013-02-28 0:00 UTC)),
            ),
        ],
    )
    .await;
    edit_determining_day(&app, &admin, "2028-06-01").await;
    assert_eq!(
        bounds(&app, "Leap").await,
        (
            Some(datetime!(2016-06-01 0:00 UTC)),
            Some(datetime!(2013-02-28 0:00 UTC))
        )
    );

    // Moving onto 29 Feb falls back to 28 Feb where the year has none.
    set_categories(
        &app,
        &[(
            "Onto",
            Some(datetime!(2016-06-01 0:00 UTC)),
            Some(datetime!(2013-06-01 0:00 UTC)),
        )],
    )
    .await;
    edit_determining_day(&app, &admin, "2028-02-29").await;
    assert_eq!(
        bounds(&app, "Onto").await,
        (
            Some(datetime!(2016-02-29 0:00 UTC)),
            Some(datetime!(2013-02-28 0:00 UTC))
        )
    );
}

#[tokio::test]
async fn the_settings_no_longer_lock_the_determining_day() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    assert!(
        settings(&app, &admin)
            .await
            .get("determining_day_locked")
            .is_none()
    );
}

#[tokio::test]
async fn the_id_never_changes_and_a_differing_file_is_flagged() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::started_with_competition_file(db, "anderer-wettkampf").await;
    let admin = app.create_admin().await;
    let body = settings(&app, &admin).await;
    assert_eq!(body["file_id_mismatch"], "anderer-wettkampf");

    let mut competition = body["competition"].clone();
    let id = competition["id"].clone();
    competition["id"] = json!("neue-id");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(settings(&app, &admin).await["competition"]["id"], id);
}

#[tokio::test]
async fn date_only_bounds_from_the_category_form_move_too() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    set_determining_day(&app, "2027-06-01").await;
    set_categories(&app, &[("Form", None, None)]).await;
    sqlx::query("UPDATE categories SET from_birthday = '2012-06-01', to_birthday = 'kaputt'")
        .execute(&app.db)
        .await
        .unwrap();
    edit_determining_day(&app, &admin, "2027-07-15").await;
    let (from, to): (String, String) =
        sqlx::query_as("SELECT from_birthday, to_birthday FROM categories")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(from, "2012-07-15T00:00:00Z");
    assert_eq!(to, "kaputt");
}
