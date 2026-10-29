//! Integration tests for the info board's photos: from a local folder and
//! from a (fake) Nextcloud over WebDAV.

mod common;

use std::{
    fs::File,
    path::Path,
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use common::TestApp;
use nrw_freestyle_cup_registration::photos::PhotosSettings;
use serde_json::{Value, json};
use uuid::Uuid;

/// A folder removed again when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cup-test-photos-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// Writes a file, changed `minutes_ago`.
    fn write(&self, name: &str, contents: &str, minutes_ago: u64) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        let modified = SystemTime::now() - Duration::from_secs(minutes_ago * 60);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn local(folder: &Folder) -> PhotosSettings {
    PhotosSettings {
        source: folder.path().to_string_lossy().into_owned(),
        user: None,
        password: None,
    }
}

/// Follows `/api/photos/newest` or `/random` to the image.
async fn follow(app: &TestApp, uri: &str) -> (String, String) {
    let redirect = app.get(uri, None).await;
    assert_eq!(redirect.status, StatusCode::FOUND, "{:?}", redirect.body);
    assert_eq!(redirect.headers[header::CACHE_CONTROL], "no-store");
    let location = redirect.headers[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    let image = app.get(&location, None).await;
    assert_eq!(image.status, StatusCode::OK);
    assert!(
        image.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let content_type = image.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    (image.body.as_str().unwrap().to_owned(), content_type)
}

#[tokio::test]
async fn without_a_source_there_are_no_photos() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let overview = app.get("/api/query/get_photos", None).await;
    assert_eq!(
        overview.body,
        json!({ "configured": false, "newest": null, "recent": 0, "error": null })
    );
    assert_eq!(
        app.get("/api/photos/newest", None).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_newest_photo_of_a_local_folder_and_its_subfolders() {
    let db = common::in_memory_migrated().await;
    let folder = Folder::new();
    folder.write("a.jpg", "older", 10);
    folder.write("Freitag/b.png", "newest", 1);
    folder.write("notes.txt", "no image", 0);
    folder.write("Freitag/tief/c.jpg", "too deep", 0);
    let app = TestApp::with_photos(db, local(&folder)).await;
    let admin = app.create_admin().await;
    app.photos.refresh().await.unwrap();

    let overview = app.get("/api/query/get_photos", None).await.body;
    assert_eq!(overview["configured"], true);
    assert_eq!(overview["recent"], 2);
    let newest_id = overview["newest"]["id"].clone();
    assert_eq!(
        overview["newest"]["url"],
        format!("/api/photos/{}", newest_id.as_str().unwrap())
    );
    assert_eq!(
        follow(&app, "/api/photos/newest").await,
        ("newest".to_owned(), "image/png".to_owned())
    );
    let (random, _) = follow(&app, "/api/photos/random").await;
    assert!(["older", "newest"].contains(&random.as_str()), "{random}");

    // Hidden, the next newest shows.
    let hide = |cookie: String, id: Value| {
        let app = &app;
        async move {
            app.post(
                "/api/command/set_photo_hidden",
                json!({ "id": id, "hidden": true }),
                Some(&cookie),
            )
            .await
            .status
        }
    };
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    assert_eq!(hide(owner, newest_id.clone()).await, StatusCode::FORBIDDEN);
    assert_eq!(
        hide(admin.clone(), json!("unknown")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(hide(admin, newest_id.clone()).await, StatusCode::OK);
    assert_eq!(follow(&app, "/api/photos/newest").await.0, "older");
    let hidden = app
        .get(
            &format!("/api/photos/{}", newest_id.as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    assert_eq!(
        app.get("/api/query/get_photos", None).await.body["recent"],
        1
    );
}

#[tokio::test]
async fn a_missing_folder_is_an_error_for_admins_only() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_photos(
        db,
        PhotosSettings {
            source: "/does/not/exist".into(),
            user: None,
            password: None,
        },
    )
    .await;
    let admin = app.create_admin().await;
    assert!(app.photos.refresh().await.is_err());
    let public = app.get("/api/query/get_photos", None).await.body;
    assert_eq!(public["error"], Value::Null);
    let for_admins = app.get("/api/query/get_photos", Some(&admin)).await.body;
    assert!(
        for_admins["error"]
            .as_str()
            .unwrap()
            .contains("/does/not/exist")
    );
}

const FOLDER: &str = "/remote.php/dav/files/anna/Fotos/";

/// A WebDAV entry: its href and, for a file, when it changed, its type and
/// Nextcloud's file id.
type DavEntry<'a> = (&'a str, Option<(&'a str, &'a str, &'a str)>);

fn multistatus(responses: &[DavEntry]) -> String {
    let responses: String = responses
        .iter()
        .map(|(href, file)| match file {
            Some((modified, content_type, file_id)) => format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop>\
                 <d:getlastmodified>{modified}</d:getlastmodified>\
                 <d:getcontenttype>{content_type}</d:getcontenttype><d:resourcetype/>\
                 <oc:fileid>{file_id}</oc:fileid></d:prop>\
                 <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            ),
            None => format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop>\
                 <d:resourcetype><d:collection/></d:resourcetype></d:prop>\
                 <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            ),
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">{responses}</d:multistatus>"#
    )
}

/// A Nextcloud folder `Fotos` of user anna (app password "geheim"), with a
/// photo in it and a newer one in the subfolder `Freitag`. Previews exist
/// only for file id 2.
async fn fake_nextcloud(request: Request) -> Response {
    let expected = format!("Basic {}", base64_encode(b"anna:geheim"));
    if request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|v| v.to_str().unwrap())
        != Some(expected.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    match (request.method().as_str(), path.as_str()) {
        ("PROPFIND", FOLDER) => multistatus(&[
            (FOLDER, None),
            (
                "/remote.php/dav/files/anna/Fotos/K%C3%BCr%201.jpg",
                Some(("Tue, 13 Oct 2026 10:00:00 GMT", "image/jpeg", "1")),
            ),
            ("/remote.php/dav/files/anna/Fotos/Freitag/", None),
        ])
        .into_response(),
        ("PROPFIND", "/remote.php/dav/files/anna/Fotos/Freitag/") => multistatus(&[
            ("/remote.php/dav/files/anna/Fotos/Freitag/", None),
            (
                "/remote.php/dav/files/anna/Fotos/Freitag/IMG_2.jpg",
                Some(("Tue, 13 Oct 2026 10:05:00 GMT", "image/jpeg", "2")),
            ),
            (
                "/remote.php/dav/files/anna/Fotos/Freitag/IMG_3.heic",
                Some(("Tue, 13 Oct 2026 10:06:00 GMT", "image/heic", "3")),
            ),
        ])
        .into_response(),
        ("GET", "/index.php/core/preview") if query.starts_with("fileId=2&") => {
            ([(header::CONTENT_TYPE, "image/jpeg")], "preview of 2").into_response()
        }
        ("GET", "/index.php/core/preview") => StatusCode::NOT_FOUND.into_response(),
        ("GET", "/remote.php/dav/files/anna/Fotos/K%C3%BCr%201.jpg") => {
            ([(header::CONTENT_TYPE, "image/jpeg")], "original of 1").into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[tokio::test]
async fn the_newest_photo_of_a_nextcloud_folder() {
    let db = common::in_memory_migrated().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let app = Router::new().fallback(any(fake_nextcloud));
        axum::serve(listener, app).await.unwrap();
    });
    let app = TestApp::with_photos(
        db,
        PhotosSettings {
            source: format!("http://{address}{}", FOLDER.trim_end_matches('/')),
            user: Some("anna".into()),
            password: Some("geheim".into()),
        },
    )
    .await;
    app.photos.refresh().await.unwrap();

    let overview = app.get("/api/query/get_photos", None).await.body;
    // HEIC is left out: browsers can't show it.
    assert_eq!(overview["recent"], 2);
    assert_eq!(overview["newest"]["modified"], "2026-10-13T10:05:00Z");
    // Scaled down by Nextcloud where it can, else the original.
    assert_eq!(
        follow(&app, "/api/photos/newest").await,
        ("preview of 2".to_owned(), "image/jpeg".to_owned())
    );
    let all = app.photos.recent();
    let (body, _) = follow_id(&app, &all[1].url).await;
    assert_eq!(body, "original of 1");
}

async fn follow_id(app: &TestApp, url: &str) -> (String, StatusCode) {
    let image = app.get(url, None).await;
    (
        image.body.as_str().unwrap_or_default().to_owned(),
        image.status,
    )
}
