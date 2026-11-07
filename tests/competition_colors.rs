//! Integration tests for the competition's primary and secondary color
//! (#287, ADR 0009): served as a stylesheet to every page, edited in the
//! admin UI, set in the competition file.

mod common;

use axum::http::{StatusCode, header};
use common::{TestApp, TestResponse};
use serde_json::{Value, json};

const COLORS_URL: &str = "/api/competition/colors.css";

/// The stylesheet's text.
async fn stylesheet(app: &TestApp) -> String {
    let response = app.get_raw(COLORS_URL, &[]).await;
    assert_eq!(response.status, StatusCode::OK);
    String::from_utf8(response.bytes).unwrap()
}

async fn settings(app: &TestApp, cookie: &str) -> Value {
    let response = app
        .get("/api/query/get_competition_settings", Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

/// Saves "Wettkampf bearbeiten" with the given colors (JSON, `null` for
/// the default) and the rest as it is.
async fn edit_colors(
    app: &TestApp,
    cookie: &str,
    primary: Value,
    secondary: Value,
) -> TestResponse {
    let mut competition = settings(app, cookie).await["competition"].clone();
    competition["primary_color"] = primary;
    competition["secondary_color"] = secondary;
    app.post(
        "/api/command/edit_competition",
        json!({ "competition": competition, "photos": null }),
        Some(cookie),
    )
    .await
}

#[tokio::test]
async fn without_colors_the_stylesheet_leaves_the_default_colors() {
    let app = TestApp::new(common::in_memory_migrated().await).await;

    let response = app.get_raw(COLORS_URL, &[]).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let css = String::from_utf8(response.bytes).unwrap();
    assert!(!css.contains("--competition-"), "{css}");
}

#[tokio::test]
async fn colors_saved_in_the_admin_ui_reach_every_page() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let response = edit_colors(&app, &admin, json!("#C05"), json!("#1A6B3C")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let css = stylesheet(&app).await;
    assert!(
        css.contains("--competition-primary-color: #cc0055;"),
        "{css}"
    );
    assert!(
        css.contains("--competition-secondary-color: #1a6b3c;"),
        "{css}"
    );
    let competition = settings(&app, &admin).await["competition"].clone();
    assert_eq!(competition["primary_color"], "#cc0055");
    assert_eq!(competition["secondary_color"], "#1a6b3c");
    let status = app.get("/api/query/get_system_status", None).await.body;
    assert_eq!(status["competition"]["primary_color"], "#cc0055");

    // Back to the default for one of them.
    let response = edit_colors(&app, &admin, Value::Null, json!("#1a6b3c")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let css = stylesheet(&app).await;
    assert!(!css.contains("--competition-primary-color"), "{css}");
    assert!(
        css.contains("--competition-secondary-color: #1a6b3c;"),
        "{css}"
    );
}

#[tokio::test]
async fn only_hex_colors_are_saved() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    edit_colors(&app, &admin, json!("#cc0055"), Value::Null).await;
    let before = stylesheet(&app).await;

    for color in [
        "red",
        "#12345",
        "#002d5680",
        "",
        "#002d56; } body { display: none",
    ] {
        let response = edit_colors(&app, &admin, json!(color), Value::Null).await;
        assert!(!response.status.is_success(), "{color}");
        let response = edit_colors(&app, &admin, Value::Null, json!(color)).await;
        assert!(!response.status.is_success(), "{color}");
    }
    assert_eq!(stylesheet(&app).await, before);
}

#[tokio::test]
async fn browsers_revalidate_the_colors_and_get_new_ones_after_a_change() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let default = app.get_raw(COLORS_URL, &[]).await;
    assert_eq!(default.headers[header::CACHE_CONTROL], "no-cache");
    let default_etag = default.headers[header::ETAG].to_str().unwrap().to_string();
    let unchanged = app
        .get_raw(COLORS_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(unchanged.status, StatusCode::NOT_MODIFIED);
    assert!(unchanged.bytes.is_empty());

    edit_colors(&app, &admin, json!("#cc0055"), Value::Null).await;
    let changed = app
        .get_raw(COLORS_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert!(
        String::from_utf8(changed.bytes)
            .unwrap()
            .contains("#cc0055")
    );
    let etag = changed.headers[header::ETAG].to_str().unwrap().to_string();
    assert_ne!(etag, default_etag);
    let response = app
        .get_raw(COLORS_URL, &[(header::IF_NONE_MATCH, &etag)])
        .await;
    assert_eq!(response.status, StatusCode::NOT_MODIFIED);

    edit_colors(&app, &admin, Value::Null, Value::Null).await;
    let reset = app
        .get_raw(COLORS_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(reset.status, StatusCode::NOT_MODIFIED, "the default again");
}

const COMPETITION: &str = include_str!("fixtures/competition.toml");

/// The test competition file with `lines` added to its `[competition]`.
fn competition_file_with(lines: &str) -> String {
    COMPETITION.replacen(
        "max_fee_per_rider = 45\n",
        &format!("max_fee_per_rider = 45\n{lines}\n"),
        1,
    )
}

async fn import(app: &TestApp, cookie: &str, file: &str) -> TestResponse {
    app.post(
        "/api/command/import_competition",
        json!({ "file": file }),
        Some(cookie),
    )
    .await
}

async fn preview(app: &TestApp, cookie: &str, file: &str) -> TestResponse {
    app.post(
        "/api/command/preview_competition_import",
        json!({ "file": file }),
        Some(cookie),
    )
    .await
}

#[tokio::test]
async fn a_competition_file_sets_the_colors() {
    let app = TestApp::without_competition(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let file = competition_file_with("primary_color = \"#C05\"\nsecondary_color = \"#1a6b3c\"");
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.body["outcome"], "created", "{:?}", response.body);

    let css = stylesheet(&app).await;
    assert!(
        css.contains("--competition-primary-color: #cc0055;"),
        "{css}"
    );
    assert!(
        css.contains("--competition-secondary-color: #1a6b3c;"),
        "{css}"
    );
}

#[tokio::test]
async fn a_replace_keeps_the_colors_unless_the_file_sets_or_resets_them() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    edit_colors(&app, &admin, json!("#cc0055"), json!("#1a6b3c")).await;
    let set = stylesheet(&app).await;

    // The test app's registration dates differ from the file's.
    let response = import(&app, &admin, COMPETITION).await;
    assert_eq!(response.body["outcome"], "replaced", "{:?}", response.body);
    assert_eq!(stylesheet(&app).await, set);
    let same = competition_file_with("primary_color = \"#CC0055\"");
    assert_eq!(
        import(&app, &admin, &same).await.body["outcome"],
        "unchanged"
    );

    let reset = competition_file_with("primary_color = \"\"");
    let response = preview(&app, &admin, &reset).await;
    assert_eq!(
        response.body["changes"],
        json!(["colors"]),
        "{:?}",
        response.body
    );
    let response = import(&app, &admin, &reset).await;
    assert_eq!(response.body["outcome"], "replaced", "{:?}", response.body);
    let css = stylesheet(&app).await;
    assert!(!css.contains("--competition-primary-color"), "{css}");
    assert!(
        css.contains("--competition-secondary-color: #1a6b3c;"),
        "{css}"
    );
    // Nothing left to reset.
    assert_eq!(
        import(&app, &admin, &reset).await.body["outcome"],
        "unchanged"
    );

    let other = competition_file_with("secondary_color = \"#009036\"");
    assert_eq!(
        preview(&app, &admin, &other).await.body["changes"],
        json!(["colors"])
    );
    import(&app, &admin, &other).await;
    let css = stylesheet(&app).await;
    assert!(
        css.contains("--competition-secondary-color: #009036;"),
        "{css}"
    );
}

/// The message of a refused command.
fn message(response: &TestResponse) -> String {
    response.body["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn an_invalid_color_in_the_file_stops_the_import() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    edit_colors(&app, &admin, json!("#cc0055"), Value::Null).await;
    let before = stylesheet(&app).await;

    let file = competition_file_with("primary_color = \"#cc005\"");
    for response in [
        preview(&app, &admin, &file).await,
        import(&app, &admin, &file).await,
    ] {
        assert_ne!(response.status, StatusCode::OK);
        let message = message(&response);
        assert!(message.contains("primary_color"), "{message}");
        assert!(message.contains("#rrggbb"), "{message}");
    }
    assert_eq!(stylesheet(&app).await, before);
}

async fn export(app: &TestApp, cookie: &str) -> String {
    let response = app.get("/api/query/export_competition", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_str().unwrap().to_string()
}

#[tokio::test]
async fn colors_are_exported_and_imported_again() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let file = export(&app, &admin).await;
    assert!(!file.contains("_color"), "default colors stay out: {file}");

    edit_colors(&app, &admin, json!("#C05"), json!("#1a6b3c")).await;
    let file = export(&app, &admin).await;
    assert!(file.contains("primary_color = \"#cc0055\""), "{file}");
    assert!(file.contains("secondary_color = \"#1a6b3c\""), "{file}");

    let fresh = TestApp::without_competition(common::in_memory_migrated().await).await;
    let fresh_admin = fresh.create_admin().await;
    let response = import(&fresh, &fresh_admin, &file).await;
    assert_eq!(response.body["outcome"], "created", "{:?}", response.body);
    assert_eq!(stylesheet(&fresh).await, stylesheet(&app).await);
    assert_eq!(export(&fresh, &fresh_admin).await, file);
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "unchanged"
    );
}

#[tokio::test]
async fn in_setup_mode_the_default_colors_apply() {
    let app = TestApp::without_competition(common::in_memory_migrated().await).await;

    let css = stylesheet(&app).await;
    assert!(!css.contains("--competition-"), "{css}");
}
