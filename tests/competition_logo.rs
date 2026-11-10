//! Integration tests for the competition's logo (#286, ADR 0009): served at
//! one URL, uploaded and removed in the admin UI, set in the competition
//! file.

mod common;

use axum::http::{StatusCode, header};
use common::{TestApp, TestResponse};
use serde_json::json;

/// The logo built into the binary.
const DEFAULT_LOGO: &[u8] =
    include_bytes!("../frontend/src/public/assets/images/nrw-freestyle-cup.svg");

const LOGO_URL: &str = "/api/competition/logo";

#[tokio::test]
async fn without_a_logo_the_built_in_default_is_served() {
    let app = TestApp::new(common::in_memory_migrated().await).await;

    let response = app.get_raw(LOGO_URL, &[]).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(response.bytes, DEFAULT_LOGO);
}

/// A small, safe SVG logo.
const SVG_LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#002d56"/></svg>"##;

async fn upload(app: &TestApp, cookie: &str, file_name: &str, bytes: &[u8]) -> TestResponse {
    app.post_file(
        "/api/command/set_competition_logo",
        file_name,
        bytes,
        Some(cookie),
    )
    .await
}

#[tokio::test]
async fn an_uploaded_svg_logo_is_served_unchanged() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let response = upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let served = app.get_raw(LOGO_URL, &[]).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.headers[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(served.bytes, SVG_LOGO.as_bytes());
}

#[tokio::test]
async fn a_removed_logo_falls_back_to_the_built_in_default() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;

    let response = app
        .post(
            "/api/command/remove_competition_logo",
            json!({}),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, DEFAULT_LOGO);
}

#[tokio::test]
async fn only_admins_change_the_logo() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let cookie = app.cookie_for(user);
        let other = SVG_LOGO.replace("#002d56", "#009036");
        assert_eq!(
            upload(&app, &cookie, "logo.svg", other.as_bytes())
                .await
                .status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.post(
                "/api/command/remove_competition_logo",
                json!({}),
                Some(&cookie)
            )
            .await
            .status,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, SVG_LOGO.as_bytes());
}

/// The message of a refused command.
fn message(response: &TestResponse) -> String {
    response.body["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn unsupported_logos_are_refused_with_a_reason() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;

    let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
    for (name, bytes) in [
        ("logo.gif", &gif[..]),
        ("logo.txt", b"Kein Bild"),
        // The type comes from the content, not the name.
        ("logo.png", b"Kein Bild"),
    ] {
        let response = upload(&app, &admin, name, bytes).await;
        assert_ne!(response.status, StatusCode::OK, "{name}");
        assert!(
            message(&response).contains("PNG, JPEG, WebP oder SVG"),
            "{name}: {:?}",
            response.body
        );
    }

    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, SVG_LOGO.as_bytes());
}

#[tokio::test]
async fn svg_logos_that_could_run_scripts_or_load_things_are_refused() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;

    const NS: &str =
        r#"xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink""#;
    for unsafe_svg in [
        format!("<svg {NS}><script>alert(1)</script></svg>"),
        format!("<svg {NS}><SCRIPT>alert(1)</SCRIPT></svg>"),
        format!(r#"<svg {NS} onload="alert(1)"/>"#),
        format!(r#"<svg {NS}><circle OnClick="alert(1)"/></svg>"#),
        format!(
            r#"<svg {NS}><foreignObject><div xmlns="http://www.w3.org/1999/xhtml">Hallo</div></foreignObject></svg>"#
        ),
        format!(r#"<svg {NS}><a href="javascript:alert(1)"><circle r="1"/></a></svg>"#),
        format!(r#"<svg {NS}><a xlink:href=" JavaScript:alert(1)"><circle r="1"/></a></svg>"#),
        format!(
            r#"<svg {NS}><a><set attributeName="href" to="javascript:alert(1)"/><circle r="1"/></a></svg>"#
        ),
        format!(r#"<svg {NS}><image href="https://example.org/tracker.png"/></svg>"#),
        format!(r#"<svg {NS}><use xlink:href="other.svg#icon"/></svg>"#),
        format!(r#"<svg {NS}><a href="data:text/html,hallo"><circle r="1"/></a></svg>"#),
        format!(
            r#"<!DOCTYPE svg [<!ENTITY name "Max Müller">]><svg {NS}><text>&name;</text></svg>"#
        ),
        format!(r#"<?xml-stylesheet href="https://example.org/style.css"?><svg {NS}/>"#),
        // Links set by an animation, and files loaded by CSS.
        format!(
            r#"<svg {NS}><image><set attributeName="xlink:href" to="https://example.org/tracker.png"/></image></svg>"#
        ),
        format!(
            r##"<svg {NS}><image><animate attributeName="href" values="#a;https://example.org/b.png"/></image></svg>"##
        ),
        format!(r#"<svg {NS}><rect style="fill: URL( 'https://example.org/fill.svg#p' )"/></svg>"#),
        format!(r#"<svg {NS}><style>@import "https://example.org/style.css";</style></svg>"#),
        format!(r#"<svg {NS}><style>rect {{ fill: url(//example.org/fill.svg#p) }}</style></svg>"#),
    ] {
        let response = upload(&app, &admin, "logo.svg", unsafe_svg.as_bytes()).await;
        assert_ne!(response.status, StatusCode::OK, "{unsafe_svg}");
        assert!(
            message(&response).contains("SVG"),
            "{unsafe_svg}: {:?}",
            response.body
        );
    }
    for not_an_svg in [
        "<html><body>Hallo</body></html>".to_string(),
        r#"<svg viewBox="0 0 10 10"/>"#.to_string(),
        format!("<svg {NS}><circle"),
    ] {
        let response = upload(&app, &admin, "logo.svg", not_an_svg.as_bytes()).await;
        assert_ne!(response.status, StatusCode::OK, "{not_an_svg}");
        assert!(
            message(&response).contains("SVG"),
            "{not_an_svg}: {:?}",
            response.body
        );
    }
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, SVG_LOGO.as_bytes());

    // What drawing programs write is fine: a DOCTYPE, own namespaces, links
    // within the file and embedded images.
    for safe_svg in [
        String::from_utf8(DEFAULT_LOGO.to_vec()).unwrap(),
        format!(
            "<svg {NS}><defs><circle id=\"dot\" r=\"1\"/></defs><use xlink:href=\"#dot\"/><image href=\"data:image/png;base64,iVBORw0KGgo=\"/></svg>"
        ),
        format!(
            r##"<svg {NS}><style>.a {{ fill: url(#g) }}</style><linearGradient id="g"/><rect class="a" style="stroke: url( '#g' )"/><animate attributeName="opacity" values="0;1"/></svg>"##
        ),
    ] {
        let response = upload(&app, &admin, "logo.svg", safe_svg.as_bytes()).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, safe_svg.as_bytes());
    }
}

/// An image of one color in `format`.
fn image(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let pixels = image::RgbImage::from_pixel(width, height, image::Rgb([0, 45, 86]));
    let image = match format {
        // The JPEG encoder takes no alpha channel.
        image::ImageFormat::Jpeg => image::DynamicImage::ImageRgb8(pixels),
        _ => image::DynamicImage::ImageRgb8(pixels).to_rgba8().into(),
    };
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}

/// A JPEG with `comment` in its metadata (a COM segment after the start).
fn jpeg_with_comment(width: u32, height: u32, comment: &str) -> Vec<u8> {
    let jpeg = image(width, height, image::ImageFormat::Jpeg);
    let length = u16::try_from(comment.len() + 2).unwrap().to_be_bytes();
    let mut bytes = jpeg[..2].to_vec();
    bytes.extend([0xff, 0xfe, length[0], length[1]]);
    bytes.extend(comment.as_bytes());
    bytes.extend(&jpeg[2..]);
    bytes
}

#[tokio::test]
async fn raster_logos_are_scaled_down_and_lose_their_metadata() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let comment = "Fotografiert von Max Müller";
    let jpeg = jpeg_with_comment(300, 200, comment);
    assert!(jpeg.windows(comment.len()).any(|w| w == comment.as_bytes()));

    for (name, bytes, mime_type, size) in [
        (
            "logo.png",
            image(4000, 1000, image::ImageFormat::Png),
            "image/png",
            (3840, 960),
        ),
        (
            "logo.png",
            // Up to 3840 px stays as it is.
            image(256, 1024, image::ImageFormat::Png),
            "image/png",
            (256, 1024),
        ),
        (
            "logo.webp",
            image(4096, 2048, image::ImageFormat::WebP),
            "image/webp",
            (3840, 1920),
        ),
        ("logo.jpg", jpeg, "image/jpeg", (300, 200)),
    ] {
        let response = upload(&app, &admin, name, &bytes).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{name}: {:?}",
            response.body
        );

        let served = app.get_raw(LOGO_URL, &[]).await;
        assert_eq!(served.headers[header::CONTENT_TYPE], mime_type);
        let decoded = image::load_from_memory(&served.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), size, "{name}");
        assert!(
            !served
                .bytes
                .windows(comment.len())
                .any(|w| w == comment.as_bytes()),
            "{name} keeps its metadata"
        );
    }
}

#[tokio::test]
async fn broken_raster_logos_are_refused_with_a_reason() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let mut broken = image(10, 10, image::ImageFormat::Png);
    broken.truncate(30);
    let response = upload(&app, &admin, "logo.png", &broken).await;
    assert_ne!(response.status, StatusCode::OK);
    assert!(message(&response).contains("Bild"), "{:?}", response.body);

    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, DEFAULT_LOGO);
}

#[tokio::test]
async fn browsers_revalidate_the_logo_and_get_a_new_one_after_a_change() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let default = app.get_raw(LOGO_URL, &[]).await;
    assert_eq!(default.headers[header::CACHE_CONTROL], "no-cache");
    let default_etag = default.headers[header::ETAG].to_str().unwrap().to_string();
    let unchanged = app
        .get_raw(LOGO_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(unchanged.status, StatusCode::NOT_MODIFIED);
    assert!(unchanged.bytes.is_empty());

    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;
    let changed = app
        .get_raw(LOGO_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.bytes, SVG_LOGO.as_bytes());
    assert_eq!(changed.headers[header::CACHE_CONTROL], "no-cache");
    let etag = changed.headers[header::ETAG].to_str().unwrap().to_string();
    assert_ne!(etag, default_etag);
    let last_modified = changed.headers[header::LAST_MODIFIED]
        .to_str()
        .unwrap()
        .to_string();
    for condition in [
        (header::IF_NONE_MATCH, etag.as_str()),
        (header::IF_MODIFIED_SINCE, last_modified.as_str()),
    ] {
        let response = app.get_raw(LOGO_URL, &[condition]).await;
        assert_eq!(response.status, StatusCode::NOT_MODIFIED);
    }

    let other = SVG_LOGO.replace("#002d56", "#009036");
    upload(&app, &admin, "logo.svg", other.as_bytes()).await;
    let replaced = app
        .get_raw(LOGO_URL, &[(header::IF_NONE_MATCH, &etag)])
        .await;
    assert_eq!(replaced.status, StatusCode::OK);
    assert_eq!(replaced.bytes, other.as_bytes());

    app.post(
        "/api/command/remove_competition_logo",
        json!({}),
        Some(&admin),
    )
    .await;
    let removed = app
        .get_raw(LOGO_URL, &[(header::IF_NONE_MATCH, &default_etag)])
        .await;
    assert_eq!(
        removed.status,
        StatusCode::NOT_MODIFIED,
        "the default again"
    );
}

#[tokio::test]
async fn opened_on_its_own_an_svg_logo_runs_no_scripts() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let default = app.get_raw(LOGO_URL, &[]).await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;
    let uploaded = app.get_raw(LOGO_URL, &[]).await;

    for response in [default, uploaded] {
        let policy = response.headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(policy.contains("default-src 'none'"), "{policy}");
        assert!(policy.contains("sandbox"), "{policy}");
        assert!(!policy.contains("script-src"), "{policy}");
        assert_eq!(response.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
}

#[tokio::test]
async fn in_setup_mode_the_default_is_served_and_nothing_uploaded() {
    let app = TestApp::without_competition(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, DEFAULT_LOGO);
    let response = upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, DEFAULT_LOGO);
}

const COMPETITION: &str = include_str!("fixtures/competition.toml");

/// The test competition file with `line` added to its `[competition]`.
fn competition_file_with(line: &str) -> String {
    COMPETITION.replacen(
        "max_fee_per_rider = 45\n",
        &format!("max_fee_per_rider = 45\n{line}\n"),
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

#[tokio::test]
async fn a_competition_file_sets_an_svg_logo() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    let file = competition_file_with(&format!("logo_svg = '''\n{SVG_LOGO}\n'''"));
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "replaced");

    let served = app.get_raw(LOGO_URL, &[]).await;
    assert_eq!(served.headers[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(served.bytes, format!("{SVG_LOGO}\n").as_bytes());
}

#[tokio::test]
async fn a_replace_keeps_the_logo_unless_the_file_sets_or_resets_it() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let png = image(600, 300, image::ImageFormat::Png);
    upload(&app, &admin, "logo.png", &png).await;
    let uploaded = app.get_raw(LOGO_URL, &[]).await.bytes;

    // The test app's registration dates differ from the file's.
    let response = import(&app, &admin, COMPETITION).await;
    assert_eq!(response.body["outcome"], "replaced", "{:?}", response.body);
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, uploaded);
    let response = import(&app, &admin, COMPETITION).await;
    assert_eq!(response.body["outcome"], "unchanged", "{:?}", response.body);

    let reset = competition_file_with("logo_svg = \"\"");
    let response = preview(&app, &admin, &reset).await;
    assert_eq!(
        response.body["changes"],
        json!(["images"]),
        "{:?}",
        response.body
    );
    let response = import(&app, &admin, &reset).await;
    assert_eq!(response.body["outcome"], "replaced", "{:?}", response.body);
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, DEFAULT_LOGO);
    // Nothing left to reset.
    assert_eq!(
        import(&app, &admin, &reset).await.body["outcome"],
        "unchanged"
    );
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
async fn the_same_logo_in_the_file_changes_nothing() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    // The test app's registration dates differ from the file's.
    import(&app, &admin, COMPETITION).await;
    let file = competition_file_with(&format!("logo_svg = '''\n{SVG_LOGO}\n'''"));
    assert_eq!(
        preview(&app, &admin, &file).await.body["changes"],
        json!(["images"])
    );
    import(&app, &admin, &file).await;
    let etag = app.get_raw(LOGO_URL, &[]).await.headers[header::ETAG].clone();

    assert_eq!(
        preview(&app, &admin, &file).await.body["changes"],
        json!([])
    );
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "unchanged"
    );
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.headers[header::ETAG], etag);
}

#[tokio::test]
async fn an_unsafe_svg_in_the_file_stops_the_import() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;

    let scripted = SVG_LOGO.replace("<circle", "<script>alert(1)</script><circle");
    let file = competition_file_with(&format!("logo_svg = '''{scripted}'''"));
    for response in [
        preview(&app, &admin, &file).await,
        import(&app, &admin, &file).await,
    ] {
        assert_ne!(response.status, StatusCode::OK);
        let message = message(&response);
        assert!(message.contains("logo_svg"), "{message}");
        assert!(message.contains("<script>"), "{message}");
    }
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, SVG_LOGO.as_bytes());
}

async fn export(app: &TestApp, cookie: &str) -> String {
    let response = app.get("/api/query/export_competition", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_str().unwrap().to_string()
}

#[tokio::test]
async fn an_svg_logo_is_exported_and_imported_again() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(&app, &admin, "logo.svg", SVG_LOGO.as_bytes()).await;

    let file = export(&app, &admin).await;
    assert!(file.contains("logo_svg = "), "{file}");

    let fresh = TestApp::without_competition(common::in_memory_migrated().await).await;
    let fresh_admin = fresh.create_admin().await;
    let response = import(&fresh, &fresh_admin, &file).await;
    assert_eq!(response.body["outcome"], "created", "{:?}", response.body);
    assert_eq!(
        fresh.get_raw(LOGO_URL, &[]).await.bytes,
        SVG_LOGO.as_bytes()
    );
    assert_eq!(export(&fresh, &fresh_admin).await, file);
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "unchanged"
    );
}

#[tokio::test]
async fn a_raster_logo_is_left_out_of_the_export_with_a_note() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    upload(
        &app,
        &admin,
        "logo.png",
        &image(64, 64, image::ImageFormat::Png),
    )
    .await;
    let uploaded = app.get_raw(LOGO_URL, &[]).await.bytes;

    let file = export(&app, &admin).await;
    assert!(!file.contains("logo_svg"), "{file}");
    let note = file
        .lines()
        .take_while(|line| line.starts_with('#') || line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(note.contains("PNG"), "{note}");
    assert!(note.contains("logo"), "{note}");

    // On the same server, the file keeps the logo.
    assert_eq!(
        import(&app, &admin, &file).await.body["outcome"],
        "unchanged"
    );
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, uploaded);
}

#[tokio::test]
async fn an_svg_over_1_mib_is_accepted() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let padding = "<!-- padding -->".repeat(70_000);
    let large = SVG_LOGO.replace("</svg>", &format!("{padding}</svg>"));
    assert!(large.len() > 1024 * 1024);
    let response = upload(&app, &admin, "logo.svg", large.as_bytes()).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(app.get_raw(LOGO_URL, &[]).await.bytes, large.as_bytes());
}

#[tokio::test]
async fn a_raster_image_longer_than_4096_px_is_scaled_down_not_refused() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let response = upload(
        &app,
        &admin,
        "logo.png",
        &image(8000, 2000, image::ImageFormat::Png),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let served = image::load_from_memory(&app.get_raw(LOGO_URL, &[]).await.bytes).unwrap();
    assert_eq!((served.width(), served.height()), (3840, 960));
}
