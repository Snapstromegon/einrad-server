//! Integration tests for the info board's break image and sponsor logos
//! (#291, ADR 0010) and the URL sources of all images: fallback order, the
//! local copy of a download, sponsors, and the competition file.

mod common;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use common::{TestApp, TestResponse};
use einrad_server::{
    image_fetch::ImageFetcher,
    images::{self, DEFAULT_BREAK_SVG, PRESET_SPONSORS},
    logo::Logo,
    reloadable_sqlite::ReloadableSqlite,
};
use serde_json::{Value, json};

const DEFAULT_LOGO: &[u8] =
    include_bytes!("../frontend/src/public/assets/images/nrw-freestyle-cup.svg");
const COMPETITION: &str = include_str!("fixtures/competition.toml");

const LOGO_URL: &str = "/api/competition/logo";
const BREAK_URL: &str = "/api/competition/break-image";

const SVG_A: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#002d56"/></svg>"##;
const SVG_B: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="9" height="9" fill="#009036"/></svg>"##;

type Files = Arc<Mutex<HashMap<String, (StatusCode, Vec<u8>)>>>;

/// A web server on a local port that serves what the test puts in it.
struct Origin {
    address: SocketAddr,
    files: Files,
}

impl Origin {
    async fn start() -> Origin {
        let files: Files = Arc::default();
        async fn serve(State(files): State<Files>, request: Request) -> impl IntoResponse {
            let found = files.lock().unwrap().get(request.uri().path()).cloned();
            // `text/plain` like a raw file on GitHub: the content tells.
            found.unwrap_or((StatusCode::NOT_FOUND, Vec::new()))
        }
        let router = Router::new().fallback(serve).with_state(files.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Origin { address, files }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), (StatusCode::OK, bytes.to_vec()));
    }

    fn fail(&self, path: &str) {
        self.files.lock().unwrap().insert(
            path.to_string(),
            (StatusCode::INTERNAL_SERVER_ERROR, b"nope".to_vec()),
        );
    }
}

async fn app() -> (TestApp, String) {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    (app, admin)
}

async fn command(app: &TestApp, admin: &str, name: &str, body: Value) -> TestResponse {
    app.post(name, body, Some(admin)).await
}

async fn ok(app: &TestApp, admin: &str, name: &str, body: Value) -> Value {
    let response = command(app, admin, name, body).await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "{name}: {:?}",
        response.body
    );
    response.body
}

async fn state(app: &TestApp, admin: &str) -> Value {
    let response = app
        .get("/api/query/get_competition_images", Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

async fn served(app: &TestApp, url: &str) -> Vec<u8> {
    app.get_raw(url, &[]).await.bytes
}

async fn set_url(app: &TestApp, admin: &str, target: &str, url: &str) -> Value {
    ok(
        app,
        admin,
        "/api/command/set_image_source",
        json!({ "target": target, "url": url }),
    )
    .await
}

#[tokio::test]
async fn the_break_image_falls_back_to_the_logo_then_the_built_in_image() {
    let (app, admin) = app().await;
    assert_eq!(served(&app, BREAK_URL).await, DEFAULT_BREAK_SVG);

    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "svg": SVG_A }),
    )
    .await;
    assert_eq!(served(&app, BREAK_URL).await, SVG_A.as_bytes());

    let response = app
        .post_file(
            "/api/command/set_break_image",
            "break.svg",
            SVG_B.as_bytes(),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(served(&app, BREAK_URL).await, SVG_B.as_bytes());
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());

    ok(&app, &admin, "/api/command/remove_break_image", json!({})).await;
    assert_eq!(served(&app, BREAK_URL).await, SVG_A.as_bytes());
    ok(
        &app,
        &admin,
        "/api/command/remove_competition_logo",
        json!({}),
    )
    .await;
    assert_eq!(served(&app, BREAK_URL).await, DEFAULT_BREAK_SVG);
    assert_eq!(served(&app, LOGO_URL).await, DEFAULT_LOGO);
}

#[tokio::test]
async fn the_break_image_etag_names_the_image_that_answers() {
    let (app, admin) = app().await;
    let default = app.get_raw(BREAK_URL, &[]).await.headers[header::ETAG].clone();
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "svg": SVG_A }),
    )
    .await;
    let logo = app.get_raw(BREAK_URL, &[]).await;
    assert_ne!(logo.headers[header::ETAG], default);
    let tag = logo.headers[header::ETAG].to_str().unwrap();
    let again = app
        .get_raw(BREAK_URL, &[(header::IF_NONE_MATCH, tag)])
        .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn only_admins_change_the_break_image_and_sponsors() {
    let (app, admin) = app().await;
    let id = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor A", "svg": SVG_A }),
    )
    .await["id"]
        .clone();
    let user = app
        .create_user_with_roles("Max Müller", &["jury_lead"])
        .await;
    let cookie = app.cookie_for(user);
    for (name, body) in [
        ("/api/command/remove_break_image", json!({})),
        ("/api/command/refresh_images", json!({})),
        (
            "/api/command/add_sponsor",
            json!({ "name": "X", "svg": SVG_A }),
        ),
        (
            "/api/command/update_sponsor",
            json!({ "id": id, "name": "Y" }),
        ),
        ("/api/command/remove_sponsor", json!({ "id": id })),
        ("/api/command/reorder_sponsors", json!({ "ids": [id] })),
        (
            "/api/command/set_image_source",
            json!({ "target": "logo", "svg": SVG_B }),
        ),
    ] {
        assert_eq!(
            command(&app, &cookie, name, body).await.status,
            StatusCode::FORBIDDEN,
            "{name}"
        );
    }
    let upload = app
        .post_file(
            "/api/command/set_break_image",
            "b.svg",
            SVG_B.as_bytes(),
            Some(&cookie),
        )
        .await;
    assert_eq!(upload.status, StatusCode::FORBIDDEN);
    assert_eq!(
        app.get("/api/query/get_competition_images", Some(&cookie))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        state(&app, &admin).await["sponsors"][0]["name"],
        "Sponsor A"
    );
}

#[tokio::test]
async fn a_url_image_is_downloaded_and_served_from_the_local_copy() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.put("/break.svg", SVG_A.as_bytes());

    let url = origin.url("/break.svg");
    let response = set_url(&app, &admin, "break_image", &url).await;
    assert_eq!(response["images"]["break_image"]["source"], "url");
    assert_eq!(response["images"]["break_image"]["url"], url);
    assert_eq!(response["images"]["break_image"]["has_image"], true);
    assert_eq!(
        response["images"]["break_image"]["fetch_error"],
        Value::Null
    );

    // The origin is gone; the local copy is served, and never redirects.
    origin.fail("/break.svg");
    let raw = app.get_raw(BREAK_URL, &[]).await;
    assert_eq!(raw.status, StatusCode::OK);
    assert_eq!(raw.bytes, SVG_A.as_bytes());
    assert!(!raw.headers.contains_key(header::LOCATION));
}

#[tokio::test]
async fn a_failed_download_keeps_the_last_good_copy_and_shows_a_warning() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.put("/logo.svg", SVG_A.as_bytes());
    set_url(&app, &admin, "logo", &origin.url("/logo.svg")).await;
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());

    origin.fail("/logo.svg");
    let refreshed = ok(&app, &admin, "/api/command/refresh_images", json!({})).await;
    let logo = &refreshed["images"]["logo"];
    assert!(
        logo["fetch_error"].as_str().unwrap().contains("500"),
        "{logo}"
    );
    assert_eq!(logo["has_image"], true);
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());

    // A later good download clears the warning and takes the new image.
    origin.put("/logo.svg", SVG_B.as_bytes());
    let refreshed = ok(&app, &admin, "/api/command/refresh_images", json!({})).await;
    assert_eq!(refreshed["images"]["logo"]["fetch_error"], Value::Null);
    assert_eq!(served(&app, LOGO_URL).await, SVG_B.as_bytes());
}

#[tokio::test]
async fn without_any_good_download_the_default_applies_and_a_warning_shows() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.fail("/logo.svg");
    let response = set_url(&app, &admin, "logo", &origin.url("/logo.svg")).await;
    assert_eq!(response["images"]["logo"]["has_image"], false);
    assert!(response["images"]["logo"]["fetch_error"].is_string());
    assert_eq!(served(&app, LOGO_URL).await, DEFAULT_LOGO);

    // A sponsor without an image isn't listed.
    let id = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor A", "url": origin.url("/missing.svg") }),
    )
    .await["id"]
        .clone();
    assert_eq!(
        app.get_raw(&format!("/api/competition/sponsors/{id}/image"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let listed = app.get("/api/query/list_sponsors", None).await;
    assert_eq!(listed.body, json!([]));
}

#[tokio::test]
async fn startup_downloads_in_the_background_and_survives_unreachable_urls() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.put("/logo.svg", SVG_A.as_bytes());
    // Port 1: nothing listens.
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "break_image", "url": "http://127.0.0.1:1/break.svg" }),
    )
    .await;
    set_url(&app, &admin, "logo", &origin.url("/logo.svg")).await;
    sqlx::query("UPDATE competition_image SET data = NULL, mime_type = NULL, fetched_at = NULL")
        .execute(&app.db)
        .await
        .unwrap();
    assert_eq!(served(&app, LOGO_URL).await, DEFAULT_LOGO);

    let db = ReloadableSqlite::new(app.db.clone(), "sqlite::memory:".to_string());
    ImageFetcher::shared().refresh_in_background(db, None);
    for _ in 0..100 {
        if served(&app, LOGO_URL).await == SVG_A.as_bytes() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());
    let images = state(&app, &admin).await;
    assert!(images["break_image"]["fetch_error"].is_string());
    assert_eq!(served(&app, BREAK_URL).await, SVG_A.as_bytes());
}

#[tokio::test]
async fn downloads_get_the_checks_of_an_upload() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;

    let scripted = SVG_A.replace("<circle", "<script>alert(1)</script><circle");
    origin.put("/scripted.svg", scripted.as_bytes());
    let response = set_url(&app, &admin, "logo", &origin.url("/scripted.svg")).await;
    let error = response["images"]["logo"]["fetch_error"].as_str().unwrap();
    assert!(error.contains("script"), "{error}");
    assert_eq!(served(&app, LOGO_URL).await, DEFAULT_LOGO);

    let padding = " ".repeat(1024 * 1024);
    origin.put(
        "/big.svg",
        SVG_A
            .replace("</svg>", &format!("{padding}</svg>"))
            .as_bytes(),
    );
    let response = set_url(&app, &admin, "logo", &origin.url("/big.svg")).await;
    assert!(
        response["images"]["logo"]["fetch_error"]
            .as_str()
            .unwrap()
            .contains("1 MiB")
    );

    origin.put("/note.txt", b"Kein Bild");
    let response = set_url(&app, &admin, "logo", &origin.url("/note.txt")).await;
    assert!(
        response["images"]["logo"]["fetch_error"]
            .as_str()
            .unwrap()
            .contains("PNG, JPEG, WebP oder SVG")
    );

    // A raster image is scaled down and loses its metadata.
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(4000, 1000)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    origin.put("/large.png", &png);
    set_url(&app, &admin, "logo", &origin.url("/large.png")).await;
    let scaled = image::load_from_memory(&served(&app, LOGO_URL).await).unwrap();
    assert_eq!((scaled.width(), scaled.height()), (3840, 960));
}

#[tokio::test]
async fn urls_must_be_http_and_an_image_has_one_source() {
    let (app, admin) = app().await;
    for url in [
        "ftp://example.org/a.svg",
        "file:///etc/passwd",
        "not a url",
        "https://user:pw@example.org/a.svg",
    ] {
        let response = command(
            &app,
            &admin,
            "/api/command/set_image_source",
            json!({ "target": "logo", "url": url }),
        )
        .await;
        assert_ne!(response.status, StatusCode::OK, "{url}");
    }
    let both = command(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "url": "https://example.org/a.svg", "svg": SVG_A }),
    )
    .await;
    assert_ne!(both.status, StatusCode::OK);
    let neither = command(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo" }),
    )
    .await;
    assert_ne!(neither.status, StatusCode::OK);
    assert_eq!(state(&app, &admin).await["logo"]["source"], "default");
}

#[tokio::test]
async fn a_download_that_finishes_after_a_change_is_dropped() {
    let (app, admin) = app().await;
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "url": "http://127.0.0.1:1/a.svg" }),
    )
    .await;
    let id = images::list(&app.db).await.unwrap()[0].id;
    let stale = Logo::from_upload(SVG_A.as_bytes()).unwrap();
    // The admin switched to an inline SVG while the download ran.
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "svg": SVG_B }),
    )
    .await;
    let stored = images::store_fetched(&app.db, id, "http://127.0.0.1:1/a.svg", Ok(stale))
        .await
        .unwrap();
    assert!(!stored);
    assert_eq!(served(&app, LOGO_URL).await, SVG_B.as_bytes());
}

#[tokio::test]
async fn sponsors_are_an_ordered_list_with_names_and_links() {
    let (app, admin) = app().await;
    assert_eq!(
        app.get("/api/query/list_sponsors", None).await.body,
        json!([])
    );

    let a = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor A", "svg": SVG_A, "link": "https://example.org/a" }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let b = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor B", "svg": SVG_B }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let c = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor C" }),
    )
    .await["id"]
        .as_i64()
        .unwrap();

    let listed = app.get("/api/query/list_sponsors", None).await.body;
    let names: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    // C has no image yet.
    assert_eq!(names, ["Sponsor A", "Sponsor B"]);
    assert_eq!(listed[0]["link"], "https://example.org/a");
    assert_eq!(
        served(&app, &format!("/api/competition/sponsors/{a}/image")).await,
        SVG_A.as_bytes()
    );

    let response = app
        .post_file_with_fields(
            "/api/command/upload_sponsor_image",
            &[("id", &c.to_string())],
            "c.svg",
            SVG_A.as_bytes(),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    ok(
        &app,
        &admin,
        "/api/command/reorder_sponsors",
        json!({ "ids": [c, a, b] }),
    )
    .await;
    let listed = app.get("/api/query/list_sponsors", None).await.body;
    let names: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Sponsor C", "Sponsor A", "Sponsor B"]);

    // The order must name every sponsor once.
    assert_ne!(
        command(
            &app,
            &admin,
            "/api/command/reorder_sponsors",
            json!({ "ids": [a, b] })
        )
        .await
        .status,
        StatusCode::OK
    );

    ok(
        &app,
        &admin,
        "/api/command/update_sponsor",
        json!({ "id": b, "name": "Sponsor B2", "link": "" }),
    )
    .await;
    ok(
        &app,
        &admin,
        "/api/command/remove_sponsor",
        json!({ "id": a }),
    )
    .await;
    let listed = app.get("/api/query/list_sponsors", None).await.body;
    let names: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Sponsor C", "Sponsor B2"]);
    assert_eq!(listed[1]["link"], Value::Null);
    assert_eq!(
        app.get_raw(&format!("/api/competition/sponsors/{a}/image"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Names and links are checked.
    assert_ne!(
        command(
            &app,
            &admin,
            "/api/command/add_sponsor",
            json!({ "name": "  " })
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_ne!(
        command(
            &app,
            &admin,
            "/api/command/add_sponsor",
            json!({ "name": "X", "link": "javascript:alert(1)" })
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_built_in_sponsor_logos_are_presets_to_add() {
    let (app, admin) = app().await;
    // Nothing by default.
    assert_eq!(
        app.get("/api/query/list_sponsors", None).await.body,
        json!([])
    );
    let presets = state(&app, &admin).await["presets"].clone();
    assert_eq!(presets.as_array().unwrap().len(), 2);
    for preset in PRESET_SPONSORS {
        let id = ok(
            &app,
            &admin,
            "/api/command/add_sponsor",
            json!({ "name": preset.name, "preset": preset.id }),
        )
        .await["id"]
            .as_i64()
            .unwrap();
        assert_eq!(
            served(&app, &format!("/api/competition/sponsors/{id}/image")).await,
            preset.svg.as_bytes()
        );
    }
    assert_ne!(
        command(
            &app,
            &admin,
            "/api/command/add_sponsor",
            json!({ "name": "X", "preset": "other" })
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[test]
fn the_built_in_images_pass_the_svg_check() {
    for svg in [DEFAULT_BREAK_SVG, DEFAULT_LOGO] {
        Logo::from_upload(svg).unwrap();
    }
    for preset in PRESET_SPONSORS {
        Logo::from_upload(preset.svg.as_bytes()).unwrap();
    }
}

fn file_with(competition_lines: &str) -> String {
    COMPETITION.replacen(
        "max_fee_per_rider = 45\n",
        &format!("max_fee_per_rider = 45\n{competition_lines}\n"),
        1,
    )
}

async fn import(app: &TestApp, admin: &str, file: &str) -> TestResponse {
    command(
        app,
        admin,
        "/api/command/import_competition",
        json!({ "file": file }),
    )
    .await
}

async fn export(app: &TestApp, admin: &str) -> String {
    let response = app.get("/api/query/export_competition", Some(admin)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_competition_file_sets_images_and_sponsors_and_a_replace_keeps_them_unless_it_sets_them()
{
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.put("/logo.svg", SVG_A.as_bytes());
    let lines = format!(
        "logo_url = \"{}\"\nbreak_image_svg = '''{SVG_B}'''\n\n\
         [[competition.sponsors]]\nname = \"Sponsor A\"\nlink = \"https://example.org/a\"\nsvg = '''{SVG_A}'''\n\n\
         [[competition.sponsors]]\nname = \"Sponsor B\"\nurl = \"{}\"\n",
        origin.url("/logo.svg"),
        origin.url("/logo.svg"),
    );
    let file = insert_competition_lines(COMPETITION, &lines);
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    ImageFetcher::shared()
        .refresh(
            &ReloadableSqlite::new(app.db.clone(), "sqlite::memory:".to_string()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(served(&app, BREAK_URL).await, SVG_B.as_bytes());
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());
    let images = state(&app, &admin).await;
    assert_eq!(images["logo"]["source"], "url");
    assert_eq!(images["break_image"]["source"], "svg");
    assert_eq!(images["sponsors"][0]["name"], "Sponsor A");
    assert_eq!(images["sponsors"][1]["image"]["source"], "url");
    assert_eq!(images["sponsors"][1]["image"]["has_image"], true);

    // A file without any of them keeps all of them.
    let replaced = import(&app, &admin, COMPETITION).await;
    assert_eq!(replaced.status, StatusCode::OK, "{:?}", replaced.body);
    assert_eq!(
        state(&app, &admin).await["sponsors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(served(&app, BREAK_URL).await, SVG_B.as_bytes());

    // An empty sponsors list and an empty break image reset them.
    let reset = insert_competition_lines(COMPETITION, "break_image_svg = \"\"\nsponsors = []\n");
    let replaced = import(&app, &admin, &reset).await;
    assert_eq!(replaced.status, StatusCode::OK, "{:?}", replaced.body);
    assert_eq!(state(&app, &admin).await["sponsors"], json!([]));
    assert_eq!(served(&app, BREAK_URL).await, SVG_A.as_bytes());
}

/// Adds `lines` to the end of the `[competition]` table of the test file's
/// scalar keys (sub-tables of it follow).
fn insert_competition_lines(file: &str, lines: &str) -> String {
    file.replacen(
        "max_fee_per_rider = 45\n",
        &format!("max_fee_per_rider = 45\n{lines}\n"),
        1,
    )
}

#[tokio::test]
async fn a_file_with_a_logo_svg_and_url_or_an_unsafe_sponsor_stops_the_import() {
    let (app, admin) = app().await;
    let both = file_with(&format!(
        "logo_svg = '''{SVG_A}'''\nlogo_url = \"https://example.org/a.svg\""
    ));
    let response = import(&app, &admin, &both).await;
    assert_ne!(response.status, StatusCode::OK);
    assert!(
        response.body.to_string().contains("logo_svg and logo_url"),
        "{:?}",
        response.body
    );

    let scripted = SVG_A.replace("<circle", "<script></script><circle");
    let unsafe_sponsor = insert_competition_lines(
        COMPETITION,
        &format!("sponsors = [{{ name = \"Sponsor A\", svg = '''{scripted}''' }}]"),
    );
    assert_ne!(
        import(&app, &admin, &unsafe_sponsor).await.status,
        StatusCode::OK
    );

    let twice = insert_competition_lines(
        COMPETITION,
        "sponsors = [{ name = \"Sponsor A\" }, { name = \" sponsor a \" }]",
    );
    let response = import(&app, &admin, &twice).await;
    assert_ne!(response.status, StatusCode::OK);
    assert!(
        response.body.to_string().contains("twice"),
        "{:?}",
        response.body
    );

    let bad_url =
        insert_competition_lines(COMPETITION, "break_image_url = \"ftp://example.org/b.svg\"");
    assert_ne!(import(&app, &admin, &bad_url).await.status, StatusCode::OK);
    assert_eq!(state(&app, &admin).await["sponsors"], json!([]));
}

#[tokio::test]
async fn the_same_images_in_the_file_change_nothing() {
    let (app, admin) = app().await;
    import(&app, &admin, COMPETITION).await;
    let file = insert_competition_lines(
        COMPETITION,
        &format!(
            "break_image_svg = '''{SVG_B}'''\nsponsors = [{{ name = \"Sponsor A\", svg = '''{SVG_A}''' }}, {{ name = \"Sponsor B\", url = \"https://example.org/b.svg\" }}]"
        ),
    );
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "replaced"
    );
    let before = images::list(&app.db).await.unwrap();

    let preview = command(
        &app,
        &admin,
        "/api/command/preview_competition_import",
        json!({ "file": file }),
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    assert_eq!(preview.body["changes"], json!([]), "{:?}", preview.body);
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "unchanged"
    );
    assert_eq!(images::list(&app.db).await.unwrap(), before);
}

#[tokio::test]
async fn images_are_exported_and_imported_again_and_raster_ones_stay() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    origin.put("/b.svg", SVG_B.as_bytes());
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "svg": SVG_A }),
    )
    .await;
    set_url(&app, &admin, "break_image", &origin.url("/b.svg")).await;
    ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor A", "svg": SVG_A, "link": "https://example.org/a" }),
    )
    .await;
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(20, 10)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let raster = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor Raster" }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    app.post_file_with_fields(
        "/api/command/upload_sponsor_image",
        &[("id", &raster.to_string())],
        "r.png",
        &png,
        Some(&admin),
    )
    .await;
    let raster_bytes = served(&app, &format!("/api/competition/sponsors/{raster}/image")).await;

    let file = export(&app, &admin).await;
    assert!(file.contains("break_image_url = "), "{file}");
    assert!(!file.contains("break_image_svg"), "{file}");
    assert!(file.contains("logo_svg = "), "{file}");
    assert!(file.contains("Sponsor Raster"), "{file}");
    assert!(file.contains("the sponsor Sponsor Raster (PNG)"), "{file}");

    // Into the same database: nothing changes, the raster image stays.
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let images = state(&app, &admin).await;
    let sponsors = images["sponsors"].as_array().unwrap();
    assert_eq!(sponsors.len(), 2);
    let raster = sponsors[1]["id"].as_i64().unwrap();
    assert_eq!(
        served(&app, &format!("/api/competition/sponsors/{raster}/image")).await,
        raster_bytes
    );
    assert_eq!(export(&app, &admin).await, file);
}

#[tokio::test]
async fn a_new_url_keeps_the_old_image_until_its_first_good_download() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "logo", "svg": SVG_A }),
    )
    .await;
    let sponsor = ok(
        &app,
        &admin,
        "/api/command/add_sponsor",
        json!({ "name": "Sponsor A", "svg": SVG_A }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let sponsor_url = format!("/api/competition/sponsors/{sponsor}/image");

    origin.fail("/new.svg");
    let response = set_url(&app, &admin, "logo", &origin.url("/new.svg")).await;
    assert_eq!(response["images"]["logo"]["source"], "url");
    assert_eq!(response["images"]["logo"]["has_image"], true);
    assert!(response["images"]["logo"]["fetch_error"].is_string());
    assert_eq!(served(&app, LOGO_URL).await, SVG_A.as_bytes());

    ok(
        &app,
        &admin,
        "/api/command/set_image_source",
        json!({ "target": "sponsor", "sponsor_id": sponsor, "url": origin.url("/new.svg") }),
    )
    .await;
    assert_eq!(served(&app, &sponsor_url).await, SVG_A.as_bytes());
    assert_eq!(
        app.get("/api/query/list_sponsors", None)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );

    origin.put("/new.svg", SVG_B.as_bytes());
    ok(&app, &admin, "/api/command/refresh_images", json!({})).await;
    assert_eq!(served(&app, LOGO_URL).await, SVG_B.as_bytes());
    assert_eq!(served(&app, &sponsor_url).await, SVG_B.as_bytes());
}

/// A PNG of noise, which doesn't compress: about 4 MB at 1200 x 1200.
fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 12345u32;
    let pixels = image::RgbImage::from_fn(width, height, |_, _| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let [a, b, c, _] = seed.to_be_bytes();
        image::Rgb([a, b, c])
    });
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

#[tokio::test]
async fn a_downloaded_raster_image_may_be_larger_than_1_mib() {
    let (app, admin) = app().await;
    let origin = Origin::start().await;
    let png = noisy_png(1200, 1200);
    assert!(png.len() > 1024 * 1024);
    origin.put("/big.png", &png);
    let response = set_url(&app, &admin, "break_image", &origin.url("/big.png")).await;
    assert_eq!(
        response["images"]["break_image"]["fetch_error"],
        Value::Null,
        "{response}"
    );
    let served = image::load_from_memory(&served(&app, BREAK_URL).await).unwrap();
    assert_eq!((served.width(), served.height()), (1200, 1200));
}
