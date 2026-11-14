//! Integration tests for the network side of the command-line player
//! `cup-player` (`player::client`), with a fake sound card. They need the
//! cargo feature `player-client`: `cargo test --features player-client`.
#![cfg(feature = "player-client")]

mod common;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use axum::http::StatusCode;
use common::{PASSWORD, TestApp, email_for};
use einrad_server::player::{
    PlaybackState, PlayerStatus, ToPlayer,
    client::{self, Options, Songs},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    sync::mpsc as tokio_mpsc,
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::COOKIE},
    },
};
use url::Url;
use uuid::Uuid;

/// Every wait fails after this instead of hanging.
const TIMEOUT: Duration = Duration::from_secs(5);

/// A sound card that records the commands and reports what the real one
/// would: a cued song is loading until it's downloaded.
struct FakeOutput {
    commands: Arc<Mutex<Vec<ToPlayer>>>,
}

impl FakeOutput {
    fn start(
        songs: Songs,
    ) -> (
        Self,
        mpsc::Sender<ToPlayer>,
        tokio_mpsc::UnboundedReceiver<PlayerStatus>,
    ) {
        let (commands_tx, commands_rx) = mpsc::channel::<ToPlayer>();
        let (statuses_tx, statuses_rx) = tokio_mpsc::unbounded_channel();
        let commands = Arc::new(Mutex::new(vec![]));
        let recorded = commands.clone();
        std::thread::spawn(move || {
            let mut status = PlayerStatus::default();
            for command in commands_rx {
                match &command {
                    ToPlayer::Cue { song } => {
                        status.file = Some(song.file.clone());
                        status.state = match songs.get(&song.file) {
                            Ok(Some(_)) => PlaybackState::Ready,
                            Ok(None) => PlaybackState::Loading,
                            Err(_) => PlaybackState::Error,
                        };
                    }
                    ToPlayer::Play if status.state == PlaybackState::Ready => {
                        status.state = PlaybackState::Playing;
                    }
                    _ => {}
                }
                recorded.lock().unwrap().push(command);
                if statuses_tx.send(status.clone()).is_err() {
                    return;
                }
            }
        });
        (Self { commands }, commands_tx, statuses_rx)
    }

    /// The cues and playback commands so far, e.g. `["cue", "play"]`.
    fn playback(&self) -> Vec<&'static str> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .filter_map(|command| match command {
                ToPlayer::Cue { .. } => Some("cue"),
                ToPlayer::Play => Some("play"),
                ToPlayer::Pause => Some("pause"),
                ToPlayer::Stop => Some("stop"),
                _ => None,
            })
            .collect()
    }
}

/// Starts a player named `name` against the server at `address`.
fn start_player(
    address: SocketAddr,
    name: &str,
    id: Uuid,
    password: &str,
) -> (Songs, FakeOutput, JoinHandle<Result<(), String>>) {
    let songs = Songs::default();
    let (output, commands, statuses) = FakeOutput::start(songs.clone());
    let options = Options {
        server: Url::parse(&format!("http://{address}")).unwrap(),
        email: email_for("Admin"),
        password: password.into(),
        name: name.into(),
        id,
    };
    let task = tokio::spawn(client::run(options, songs.clone(), commands, statuses));
    (songs, output, task)
}

/// The music control's end of the websocket.
struct Controller(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl Controller {
    async fn connect(address: SocketAddr, cookie: &str) -> Self {
        let mut request = format!("ws://{address}/api/ws/player?mode=controller")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(COOKIE, HeaderValue::from_str(cookie).unwrap());
        let (socket, _) = timeout(TIMEOUT, connect_async(request))
            .await
            .unwrap()
            .unwrap();
        Self(socket)
    }

    async fn send(&mut self, message: Value) {
        self.0
            .send(Message::text(message.to_string()))
            .await
            .unwrap();
    }

    /// Skips state updates until one matches; fails after [`TIMEOUT`].
    async fn wait_state(&mut self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        let mut last = Value::Null;
        loop {
            let Ok(message) = timeout_at(deadline, self.0.next()).await else {
                panic!("no state with {what} within 5 s, last: {last:#}");
            };
            let Message::Text(text) = message.unwrap().unwrap() else {
                continue;
            };
            let state: Value = serde_json::from_str(&text).unwrap();
            if state["type"] == "state" && matches(&state) {
                return state;
            }
            last = state;
        }
    }
}

fn player<'a>(state: &'a Value, name: &str) -> Option<&'a Value> {
    state["players"]
        .as_array()?
        .iter()
        .find(|player| player["name"] == name)
}

/// An act with an uploaded song (not a real mp3). Returns the act's id.
async fn act_with_song(app: &TestApp, song: &[u8]) -> Uuid {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let act_id = app.add_single_act(&owner, club_id, "Anna").await;
    let response = app
        .post_file(
            &format!("/api/command/save_act_song?act_id={act_id}"),
            "song.mp3",
            song,
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    act_id
}

#[tokio::test]
async fn the_cli_player_downloads_the_cued_song_and_plays_it() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let song = b"ID3 not really an mp3".as_slice();
    let act_id = act_with_song(&app, song).await;
    let address = app.serve().await;
    let mut controller = Controller::connect(address, &admin).await;
    let (songs, output, _player) = start_player(address, "Tonpult", Uuid::now_v7(), PASSWORD);

    let state = controller
        .wait_state("the CLI player", |s| player(s, "Tonpult").is_some())
        .await;
    let tonpult = player(&state, "Tonpult").unwrap();
    assert_eq!(tonpult["kind"], "cli");
    let id = tonpult["id"].clone();
    controller
        .send(json!({ "type": "select_player", "player_id": id }))
        .await;
    controller
        .send(json!({ "type": "cue", "act_id": act_id, "original": true }))
        .await;
    // Cued at once (loading), then again once downloaded; the play waits.
    controller.send(json!({ "type": "play" })).await;
    controller
        .wait_state("the song playing", |s| {
            player(s, "Tonpult").is_some_and(|p| p["status"]["state"] == "playing")
        })
        .await;
    assert_eq!(output.playback(), ["cue", "cue", "play"]);
    let file = format!("{act_id}.mp3");
    assert_eq!(songs.get(&file).unwrap().as_deref(), Some(song));
}

#[tokio::test]
async fn a_rejected_login_stops_the_cli_player() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    app.create_admin().await;
    let address = app.serve().await;
    let (_, _, player) = start_player(address, "Tonpult", Uuid::now_v7(), "falsch");
    let result = timeout(TIMEOUT, player).await.unwrap().unwrap();
    assert!(result.unwrap_err().contains("rejected"));
}

#[tokio::test]
async fn a_cli_player_with_the_same_id_takes_over() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let mut controller = Controller::connect(address, &admin).await;
    let id = Uuid::now_v7();
    let (_, _, first) = start_player(address, "Tonpult", id, PASSWORD);
    controller
        .wait_state("the first player", |s| player(s, "Tonpult").is_some())
        .await;

    let (_, _, _second) = start_player(address, "Tonpult", id, PASSWORD);
    let result = timeout(TIMEOUT, first).await.unwrap().unwrap();
    assert!(result.unwrap_err().contains("took over"));
    // The second one stays.
    let state = controller
        .wait_state("one player left", |s| {
            s["players"].as_array().unwrap().len() == 1
        })
        .await;
    assert_eq!(player(&state, "Tonpult").unwrap()["connected"], true);
}
