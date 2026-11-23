//! `/songs` serves song files to logged-in users only, and only files that
//! look like songs: never other files of the data folder.

mod common;

use axum::http::{StatusCode, header};
use common::TestApp;
use uuid::Uuid;

const BYTES: &[u8] = b"0123456789abcdefghij";

/// A data folder with a song of each kind and files that are none.
async fn app_with_files() -> (TestApp, Uuid, Uuid) {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (act, version) = (Uuid::now_v7(), Uuid::now_v7());
    let data = app.data_path();
    std::fs::write(data.join(format!("{act}.mp3")), BYTES).unwrap();
    std::fs::write(data.join(format!("{act}.wav")), BYTES).unwrap();
    std::fs::write(data.join(format!("{act}.{version}.normalized.mp3")), BYTES).unwrap();
    std::fs::write(data.join(format!("{act}.toml")), "x = 1").unwrap();
    std::fs::create_dir_all(data.join("remap")).unwrap();
    std::fs::write(
        data.join("remap/x.toml"),
        "firstname = \"Max\"\nlastname = \"Müller\"\n",
    )
    .unwrap();
    std::fs::write(data.join("notes.txt"), "not a song").unwrap();
    (app, act, version)
}

#[tokio::test]
async fn without_a_login_songs_answer_401() {
    let (app, act, _) = app_with_files().await;
    let response = app.get_raw(&format!("/songs/{act}.mp3"), &[]).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_ne!(response.bytes, BYTES);
}

#[tokio::test]
async fn a_logged_in_user_gets_every_kind_of_song() {
    let (app, act, version) = app_with_files().await;
    let (cookie, _) = app.create_club_owner("Owner", "RSV").await;
    for name in [
        format!("{act}.mp3"),
        format!("{act}.wav"),
        format!("{act}.{version}.normalized.mp3"),
    ] {
        let response = app
            .get_raw(&format!("/songs/{name}"), &[(header::COOKIE, &cookie)])
            .await;
        assert_eq!(response.status, StatusCode::OK, "{name}");
        assert_eq!(response.bytes, BYTES, "{name}");
    }
    let response = app
        .get_raw(&format!("/songs/{act}.mp3"), &[(header::COOKIE, &cookie)])
        .await;
    assert_eq!(response.headers[header::CONTENT_TYPE], "audio/mpeg");
}

#[tokio::test]
async fn songs_support_range_requests() {
    let (app, act, _) = app_with_files().await;
    let (cookie, _) = app.create_club_owner("Owner", "RSV").await;
    let response = app
        .get_raw(
            &format!("/songs/{act}.mp3"),
            &[(header::COOKIE, &cookie), (header::RANGE, "bytes=2-5")],
        )
        .await;
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.bytes, b"2345");
    assert_eq!(response.headers[header::CONTENT_RANGE], "bytes 2-5/20");
}

#[tokio::test]
async fn files_that_are_no_song_answer_404_even_when_logged_in() {
    let (app, act, _) = app_with_files().await;
    let admin = app.create_admin().await;
    for path in [
        "/songs/remap/x.toml".to_string(),
        "/songs/remap%2Fx.toml".to_string(),
        "/songs/notes.txt".to_string(),
        "/songs/..%2FCargo.toml".to_string(),
        format!("/songs/{act}.toml"),
        format!("/songs/{act}.mp3.bak"),
        format!("/songs/{}.mp3", Uuid::now_v7()),
        "/songs/".to_string(),
        "/songs".to_string(),
    ] {
        let response = app.get_raw(&path, &[(header::COOKIE, &admin)]).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{path}");
    }
}
