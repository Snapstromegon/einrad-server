//! Integration tests for remote music playback over the websocket
//! `/api/ws/player`: a controller (the music control) drives the active
//! player, and "Weiter" in the timeplan cues the songs.

mod common;

use std::{net::SocketAddr, time::Duration};

use axum::http::StatusCode;
use common::TestApp;
use futures_util::{SinkExt, StreamExt};
use nrw_freestyle_cup_registration::player::Timing;
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        self, Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::COOKIE},
    },
};
use uuid::Uuid;

/// Every receive fails after this instead of hanging.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Format sent by the admin frontend.
const START: &str = "2026-04-19T09:00:00Z";

/// One end of the playback websocket, as a controller or a player.
struct Ws(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl Ws {
    /// Opens the websocket with `query`, e.g. `mode=controller`. Returns the
    /// status code if the upgrade is refused.
    async fn try_connect(
        address: SocketAddr,
        query: &str,
        cookie: Option<&str>,
    ) -> Result<Self, StatusCode> {
        let mut request = format!("ws://{address}/api/ws/player?{query}")
            .into_client_request()
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert(COOKIE, HeaderValue::from_str(cookie).unwrap());
        }
        let connected = timeout(TIMEOUT, connect_async(request))
            .await
            .expect("the handshake timed out");
        match connected {
            Ok((socket, _)) => Ok(Self(socket)),
            Err(tungstenite::Error::Http(response)) => {
                Err(StatusCode::from_u16(response.status().as_u16()).unwrap())
            }
            Err(e) => panic!("the handshake failed: {e}"),
        }
    }

    async fn connect(address: SocketAddr, query: &str, cookie: &str) -> Self {
        Self::try_connect(address, query, Some(cookie))
            .await
            .unwrap_or_else(|status| panic!("upgrade refused with {status}"))
    }

    async fn send_text(&mut self, text: &str) {
        timeout(TIMEOUT, self.0.send(Message::text(text.to_owned())))
            .await
            .expect("sending timed out")
            .unwrap();
    }

    async fn send(&mut self, message: Value) {
        self.send_text(&message.to_string()).await;
    }

    /// The next text message, parsed.
    async fn next_message(&mut self) -> Value {
        loop {
            let message = self
                .0
                .next()
                .await
                .expect("the socket closed")
                .expect("the socket failed");
            match message {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(_) | Message::Pong(_) => {}
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    /// The next message; fails after [`TIMEOUT`].
    async fn next(&mut self) -> Value {
        timeout(TIMEOUT, self.next_message())
            .await
            .expect("no message within 5 s")
    }

    /// Skips messages until one matches `matches`; fails after [`TIMEOUT`],
    /// listing the skipped messages.
    async fn wait_for(&mut self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        let mut skipped = vec![];
        loop {
            let Ok(message) = timeout_at(deadline, self.next_message()).await else {
                panic!("no {what} within 5 s, skipped: {skipped:#?}");
            };
            if matches(&message) {
                return message;
            }
            skipped.push(message);
        }
    }

    /// Controllers: skips state updates until one matches.
    async fn wait_state(&mut self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(what, |m| m["type"] == "state" && matches(m))
            .await
    }

    /// Reads until the server closes the connection: the close code, if it
    /// sent one. Fails after [`TIMEOUT`].
    async fn closed(&mut self) -> Option<u16> {
        timeout(TIMEOUT, async {
            loop {
                match self.0.next().await {
                    Some(Ok(Message::Close(frame))) => return frame.map(|f| u16::from(f.code)),
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return None,
                }
            }
        })
        .await
        .expect("the socket wasn't closed within 5 s")
    }

    /// Reads, and so answers the pings, for `duration`: the pings that came.
    async fn read_for(&mut self, duration: Duration) -> usize {
        let deadline = Instant::now() + duration;
        let mut pings = 0;
        while let Ok(message) = timeout_at(deadline, self.0.next()).await {
            let message = message
                .expect("the socket closed")
                .expect("the socket failed");
            if let Message::Ping(_) = message {
                pings += 1;
            }
        }
        pings
    }

    async fn close(mut self) {
        timeout(TIMEOUT, self.0.close(None))
            .await
            .expect("closing timed out")
            .unwrap();
    }
}

/// Connects a controller and checks its first message, the state.
async fn controller(address: SocketAddr, admin: &str) -> (Ws, Value) {
    let mut controller = Ws::connect(address, "mode=controller", admin).await;
    let state = controller.next().await;
    assert_eq!(state["type"], "state", "{state}");
    (controller, state)
}

fn find_player<'a>(state: &'a Value, name: &str) -> Option<&'a Value> {
    state["players"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
}

/// Connects a player named `name` and returns it with its id, as the
/// controller sees it.
async fn player(address: SocketAddr, admin: &str, controller: &mut Ws, name: &str) -> (Ws, Value) {
    player_as(address, admin, controller, name, name).await
}

/// Connects a player with `query_name` in the URL, shown as `name`.
async fn player_as(
    address: SocketAddr,
    admin: &str,
    controller: &mut Ws,
    query_name: &str,
    name: &str,
) -> (Ws, Value) {
    let query = format!("name={query_name}");
    player_query(address, admin, controller, &query, name).await
}

/// Connects a new player with `query` after `mode=player&`, shown as
/// `name`.
async fn player_query(
    address: SocketAddr,
    admin: &str,
    controller: &mut Ws,
    query: &str,
    name: &str,
) -> (Ws, Value) {
    let query = format!("mode=player&{query}");
    let mut player = Ws::connect(address, &query, admin).await;
    // Every player starts inactive, with the songs coming up.
    assert_eq!(
        player.next().await,
        json!({ "type": "active", "active": false })
    );
    assert_eq!(player.next().await["type"], "prefetch");
    let state = controller
        .wait_state(&format!("state with player {name}"), |s| {
            find_player(s, name).is_some()
        })
        .await;
    let id = find_player(&state, name).unwrap()["id"].clone();
    (player, id)
}

/// Selects `player_id` (`null`: none) and waits until the controller sees it.
async fn select(controller: &mut Ws, player_id: &Value) {
    controller
        .send(json!({ "type": "select_player", "player_id": player_id }))
        .await;
    controller
        .wait_state("state with the selected player", |s| {
            s["active_player"] == *player_id
        })
        .await;
}

fn connected(state: &Value, name: &str) -> Option<bool> {
    find_player(state, name).map(|p| p["connected"].as_bool().unwrap())
}

/// Players that lose their connection are removed after this.
fn short_grace() -> Timing {
    Timing {
        reconnect_grace: Duration::from_millis(300),
        ..Timing::default()
    }
}

fn status(state: &str, position: f64) -> Value {
    json!({
        "type": "status",
        "state": state,
        "position": position,
        "duration": 180.0,
        "volume": 1.0,
        "file": null,
        "error": null,
        "unlocked": true,
    })
}

/// The song of a single act by `firstname` that has no normalized copy (yet).
fn original_song(act_id: Uuid, firstname: &str) -> Value {
    json!({
        "act_id": act_id,
        "file": format!("{act_id}.mp3"),
        "url": format!("/songs/{act_id}.mp3"),
        "label": format!("{firstname} Muster"),
        "original": true,
    })
}

fn cue(song: Value) -> Value {
    json!({ "type": "cue", "song": song })
}

fn act_ids(songs: &Value) -> Vec<String> {
    songs
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["act_id"].as_str().unwrap().to_string())
        .collect()
}

fn ids(acts: &[Uuid]) -> Vec<String> {
    acts.iter().map(Uuid::to_string).collect()
}

/// Single acts in EWU13, in this order, each with an uploaded (fake) mp3.
/// The timeplan is just their category, instead of the seeded one.
async fn acts_with_songs(app: &TestApp, admin: &str, firstnames: &[&str]) -> Vec<Uuid> {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let mut acts = vec![];
    for (order, firstname) in firstnames.iter().enumerate() {
        let act_id = app.add_single_act(&owner, club_id, firstname).await;
        let response = app
            .post(
                "/api/command/set_act_order",
                json!({ "act_id": act_id, "order": order + 1 }),
                Some(admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        // Not a real mp3: its normalization fails in the background, and
        // the players get the original.
        let response = app
            .post_file(
                &format!("/api/command/save_act_song?act_id={act_id}"),
                "song.mp3",
                b"data",
                Some(&owner),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        acts.push(act_id);
    }
    // Like deleting every entry, but in one go.
    sqlx::query("DELETE FROM timeplan")
        .execute(&app.db)
        .await
        .unwrap();
    let entry = json!({
        "earliest_start_time": START,
        "duration_seconds": null,
        "label": null,
        "category": "EWU13",
    });
    let response = app
        .post("/api/command/add_timeplan_entry", entry, Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    acts
}

/// Lets the (failing) normalization of an act's song finish, then pretends
/// it worked. Returns the normalized file.
async fn pretend_normalized(app: &TestApp, act_id: Uuid) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: Option<String> =
            sqlx::query_scalar("SELECT normalization_status FROM acts WHERE id = ?")
                .bind(act_id)
                .fetch_one(&app.db)
                .await
                .unwrap();
        if status.as_deref() != Some("pending") {
            break;
        }
        assert!(Instant::now() < deadline, "the normalization didn't finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    sqlx::query("UPDATE acts SET normalization_status = 'done' WHERE id = ?")
        .bind(act_id)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT song_file_normalized FROM acts WHERE id = ?")
        .bind(act_id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// The song of a single act by `firstname`, normalized into `file`.
fn normalized_song(act_id: Uuid, firstname: &str, file: &str) -> Value {
    json!({
        "act_id": act_id,
        "file": file,
        "url": format!("/songs/{file}"),
        "label": format!("{firstname} Muster"),
        "original": false,
    })
}

async fn forward(app: &TestApp, admin: &str) {
    let response = app
        .post("/api/command/timeplan_forward", json!(null), Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// Anna and Berta with songs; Tonpult is the active player. Anna's song
/// plays again while she is judged (or is paused mid-song, as `state` says)
/// when Berta's act starts, so Berta's song waits.
struct BertaWaiting {
    address: SocketAddr,
    controller: Ws,
    tonpult: Ws,
    tonpult_id: Value,
    anna: Uuid,
    berta: Uuid,
}

async fn berta_waiting(app: &TestApp, admin: &str, state: &str) -> BertaWaiting {
    let acts = acts_with_songs(app, admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, admin).await;
    let (mut tonpult, id) = player(address, admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;
    let message = tonpult.wait_for("Anna's cue", |m| m["type"] == "cue").await;
    assert_eq!(message, cue(original_song(anna, "Anna")));
    // Warm-up, Anna's act, then she is judged and Berta's song is cued.
    for _ in 0..3 {
        forward(app, admin).await;
    }
    tonpult
        .wait_for("Berta's cue", |m| m == &cue(original_song(berta, "Berta")))
        .await;
    // Her song again, meanwhile.
    controller
        .send(json!({ "type": "cue", "act_id": anna, "original": false }))
        .await;
    tonpult
        .wait_for("Anna's cue", |m| m == &cue(original_song(anna, "Anna")))
        .await;
    tonpult.send(status(state, 1.0)).await;
    controller
        .wait_state(&format!("state with Tonpult {state}"), |s| {
            find_player(s, "Tonpult").unwrap()["status"]["state"] == state
        })
        .await;
    // Berta starts while it still plays.
    forward(app, admin).await;
    let state = controller
        .wait_state("state with Berta pending", |s| !s["pending_cue"].is_null())
        .await;
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    assert_eq!(state["cue"], original_song(anna, "Anna"));
    BertaWaiting {
        address,
        controller,
        tonpult,
        tonpult_id: id,
        anna,
        berta,
    }
}

#[tokio::test]
async fn upgrade_requires_an_admin() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let user = app.cookie_for(app.create_user("Alex", false).await);
    let address = app.serve().await;

    for query in ["mode=controller", "mode=player&name=Tonpult"] {
        let anonymous = Ws::try_connect(address, query, None).await;
        assert_eq!(anonymous.err(), Some(StatusCode::UNAUTHORIZED), "{query}");
        let not_admin = Ws::try_connect(address, query, Some(&user)).await;
        assert_eq!(not_admin.err(), Some(StatusCode::FORBIDDEN), "{query}");
        let socket = Ws::try_connect(address, query, Some(&admin)).await;
        assert!(socket.is_ok(), "{query}");
    }
    let unknown_mode = Ws::try_connect(address, "mode=dj", Some(&admin)).await;
    assert_eq!(unknown_mode.err(), Some(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn controller_sees_players_come_and_go() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    app.player_hub.set_timing(short_grace());
    let address = app.serve().await;

    let (mut controller, state) = controller(address, &admin).await;
    assert_eq!(
        state,
        json!({
            "type": "state",
            "players": [],
            "active_player": null,
            "cue": null,
            "pending_cue": null,
            "volume": 1.0,
            "original_acts": [],
            "queue": [],
        })
    );

    // The name is trimmed.
    let (mut tonpult, id) =
        player_as(address, &admin, &mut controller, "%20Tonpult%20", "Tonpult").await;
    let state = serde_json::to_value(app.player_hub.subscribe().0).unwrap();
    assert_eq!(
        find_player(&state, "Tonpult").unwrap(),
        &json!({
            "id": id,
            "name": "Tonpult",
            "kind": "web",
            "connected": true,
            "status": {
                "state": "idle",
                "position": 0.0,
                "duration": null,
                "volume": 1.0,
                "file": null,
                "error": null,
                "unlocked": true,
            },
        })
    );
    // Without a name, a player gets a default one.
    let (unnamed, _) = player_as(address, &admin, &mut controller, "", "Web-Player").await;
    let state = serde_json::to_value(app.player_hub.subscribe().0).unwrap();
    assert_eq!(state["players"].as_array().unwrap().len(), 2);

    // Unknown messages are ignored; the status shows in the controller.
    tonpult.send_text("kein json").await;
    tonpult
        .send(json!({
            "type": "status",
            "state": "paused",
            "position": 12.5,
            "duration": 180.0,
            "volume": 0.8,
            "file": "song.mp3",
            "error": null,
            "unlocked": false,
        }))
        .await;
    let state = controller
        .wait_state("state with Tonpult paused", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["state"] == "paused"
        })
        .await;
    assert_eq!(
        find_player(&state, "Tonpult").unwrap()["status"],
        json!({
            "state": "paused",
            "position": 12.5,
            "duration": 180.0,
            "volume": 0.8,
            "file": "song.mp3",
            "error": null,
            "unlocked": false,
        })
    );

    // A player that closes stays for a while, in case it comes back.
    unnamed.close().await;
    controller
        .wait_state("state with Web-Player disconnected", |s| {
            connected(s, "Web-Player") == Some(false)
        })
        .await;
    let state = controller
        .wait_state("state without Web-Player", |s| {
            find_player(s, "Web-Player").is_none()
        })
        .await;
    assert_eq!(connected(&state, "Tonpult"), Some(true));
}

#[tokio::test]
async fn selecting_a_player_activates_it() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;

    select(&mut controller, &id).await;

    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": true })
    );
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "volume", "volume": 1.0 })
    );
    // Nothing to cue without acts, but the songs coming up are sent.
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "prefetch", "songs": [] })
    );

    // Back to playing in the music control itself: the player stops.
    select(&mut controller, &Value::Null).await;
    assert_eq!(tonpult.next().await, json!({ "type": "stop" }));
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": false })
    );
}

#[tokio::test]
async fn timeplan_steps_cue_the_songs_without_starting_them() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;

    // Selecting a player cues the act due next.
    select(&mut controller, &id).await;
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": true })
    );
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "volume", "volume": 1.0 })
    );
    let prefetch = tonpult.next().await;
    assert_eq!(prefetch["type"], "prefetch");
    assert_eq!(act_ids(&prefetch["songs"]), ids(&[anna, berta]));
    assert_eq!(tonpult.next().await, cue(original_song(anna, "Anna")));
    controller
        .wait_state("state with Anna cued", |s| {
            s["cue"] == original_song(anna, "Anna")
        })
        .await;

    // Each step sends the songs coming up first; the song only changes when
    // the next act is due. Ending an act stops the music before that. A step
    // without a new song is followed directly by the next step's prefetch.
    let steps = [
        // The category starts with its warm-up.
        (false, vec![anna, berta], None),
        // Anna starts.
        (false, vec![anna, berta], None),
        // Anna ends: Berta is due.
        (true, vec![berta], Some(cue(original_song(berta, "Berta")))),
        // Berta starts.
        (false, vec![berta], None),
        // Berta ends, and with her the category: nothing left to play.
        (true, vec![], Some(json!({ "type": "clear" }))),
    ];
    for (stops, songs, message) in steps {
        forward(&app, &admin).await;
        if stops {
            assert_eq!(tonpult.next().await, json!({ "type": "stop" }));
        }
        let prefetch = tonpult.next().await;
        assert_eq!(prefetch["type"], "prefetch", "{prefetch}");
        assert_eq!(act_ids(&prefetch["songs"]), ids(&songs));
        if let Some(message) = message {
            assert_eq!(tonpult.next().await, message);
        }
    }
    let state = controller
        .wait_state("state without a cue", |s| s["cue"].is_null())
        .await;
    assert_eq!(state["pending_cue"], Value::Null);
    assert_eq!(state["active_player"], id);
}

#[tokio::test]
async fn ending_an_act_stops_its_music() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;
    tonpult
        .wait_for("Anna's cue", |m| m == &cue(original_song(anna, "Anna")))
        .await;
    // Warm-up, then Anna starts and her music plays.
    forward(&app, &admin).await;
    forward(&app, &admin).await;
    tonpult.send(status("playing", 1.0)).await;
    controller
        .wait_state("state with Tonpult playing", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["state"] == "playing"
        })
        .await;

    // Judging starts in silence, and Berta's song comes right away.
    forward(&app, &admin).await;
    let message = tonpult
        .wait_for("the stop", |m| m["type"] != "prefetch")
        .await;
    assert_eq!(message, json!({ "type": "stop" }));
    let message = tonpult
        .wait_for("Berta's cue", |m| m["type"] != "prefetch")
        .await;
    assert_eq!(message, cue(original_song(berta, "Berta")));
    let state = controller
        .wait_state("state with Berta cued", |s| {
            s["cue"] == original_song(berta, "Berta")
        })
        .await;
    assert_eq!(state["pending_cue"], Value::Null);
}

#[tokio::test]
async fn a_new_song_waits_until_the_playing_one_stops() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let BertaWaiting {
        mut controller,
        mut tonpult,
        anna,
        berta,
        ..
    } = berta_waiting(&app, &admin, "playing").await;

    // Still playing: nothing changes.
    tonpult.send(status("playing", 10.0)).await;
    let state = controller
        .wait_state("state at 10 s", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["position"] == 10.0
        })
        .await;
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    assert_eq!(state["cue"], original_song(anna, "Anna"));

    // The song ends: Berta's song is cued.
    tonpult.send(status("ended", 180.0)).await;
    let message = tonpult
        .wait_for("Berta's cue", |m| m["type"] != "prefetch")
        .await;
    assert_eq!(message, cue(original_song(berta, "Berta")));
    let state = controller
        .wait_state("state with Berta cued", |s| s["pending_cue"].is_null())
        .await;
    assert_eq!(state["cue"], original_song(berta, "Berta"));
}

#[tokio::test]
async fn playback_commands_go_only_to_the_active_player() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, tonpult_id) = player(address, &admin, &mut controller, "Tonpult").await;
    let (mut laptop, _) = player(address, &admin, &mut controller, "Laptop").await;
    select(&mut controller, &tonpult_id).await;
    tonpult
        .wait_for("the prefetch after selecting", |m| m["type"] == "prefetch")
        .await;
    // The other player only hears about the songs coming up.
    assert_eq!(laptop.next().await["type"], "prefetch");

    for command in [
        json!({ "type": "play" }),
        json!({ "type": "pause" }),
        json!({ "type": "stop" }),
        json!({ "type": "seek", "position": 12.5 }),
        json!({ "type": "seek", "position": -3.0 }),
        json!({ "type": "volume", "volume": 0.5 }),
        json!({ "type": "volume", "volume": 3.0 }),
    ] {
        controller.send(command).await;
    }

    for expected in [
        json!({ "type": "play" }),
        json!({ "type": "pause" }),
        json!({ "type": "stop" }),
        json!({ "type": "seek", "position": 12.5 }),
        json!({ "type": "seek", "position": 0.0 }),
        json!({ "type": "volume", "volume": 0.5 }),
        json!({ "type": "volume", "volume": 1.0 }),
    ] {
        assert_eq!(tonpult.next().await, expected);
    }
    controller
        .wait_state("state with volume 0.5", |s| s["volume"] == 0.5)
        .await;
    controller
        .wait_state("state with volume 1", |s| s["volume"] == 1.0)
        .await;
    // Every player gets the prefetch of a timeplan step. Messages to a
    // player keep their order, so had the other player got any of the
    // commands, they would come first.
    forward(&app, &admin).await;
    assert_eq!(laptop.next().await["type"], "prefetch");
    assert_eq!(tonpult.next().await["type"], "prefetch");
}

#[tokio::test]
async fn closing_the_active_player_removes_it_after_a_while() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    app.player_hub.set_timing(short_grace());
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;

    tonpult.close().await;

    // It stays active while it may come back.
    let state = controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;
    assert_eq!(state["active_player"], id);
    let state = controller
        .wait_state("state without players", |s| {
            s["players"].as_array().unwrap().is_empty()
        })
        .await;
    assert_eq!(state["active_player"], Value::Null);
    // It can't be selected any more.
    controller
        .send(json!({ "type": "select_player", "player_id": id }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(error["message"], "Dieser Player ist nicht mehr verbunden.");
}

/// The award ceremony queue as the controllers see it: `(label, played)`.
fn queue(state: &Value) -> Vec<(String, bool)> {
    state["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["label"].as_str().unwrap().to_owned(),
                e["played"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_award_queue_plays_songs_on_demand() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;

    // Without a player, a music control plays it itself and marks it.
    for act in [anna, berta] {
        controller
            .send(json!({ "type": "queue_add", "act_id": act, "original": true }))
            .await;
    }
    let state = controller
        .wait_state("two queued songs", |s| {
            s["queue"].as_array().unwrap().len() == 2
        })
        .await;
    assert_eq!(
        queue(&state),
        [
            ("Anna Muster".to_owned(), false),
            ("Berta Muster".to_owned(), false)
        ]
    );
    let berta_entry = state["queue"][1]["id"].clone();
    controller
        .send(json!({ "type": "queue_move", "id": berta_entry, "to": 0 }))
        .await;
    controller
        .wait_state("Berta first", |s| s["queue"][0]["id"] == berta_entry)
        .await;
    controller
        .send(json!({ "type": "queue_play", "id": berta_entry }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(error["message"], "Kein Player ausgewählt.");

    // On the active player: loaded and started, stopping what played.
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;
    controller
        .send(json!({ "type": "queue_play", "id": berta_entry }))
        .await;
    tonpult
        .wait_for("Berta's cue", |m| {
            m["type"] == "cue" && m["song"]["act_id"] == berta.to_string()
        })
        .await;
    assert_eq!(tonpult.next().await, json!({ "type": "play" }));
    let state = controller
        .wait_state("Berta played", |s| s["queue"][0]["played"] == true)
        .await;
    assert_eq!(state["cue"]["act_id"], berta.to_string());

    let anna_entry = state["queue"][1]["id"].clone();
    controller
        .send(json!({ "type": "queue_played", "id": anna_entry }))
        .await;
    let state = controller
        .wait_state("Anna played", |s| s["queue"][1]["played"] == true)
        .await;
    assert_eq!(
        queue(&state),
        [
            ("Berta Muster".to_owned(), true),
            ("Anna Muster".to_owned(), true)
        ]
    );
    controller
        .send(json!({ "type": "queue_remove", "id": anna_entry }))
        .await;
    controller
        .wait_state("one song left", |s| {
            s["queue"].as_array().unwrap().len() == 1
        })
        .await;
    controller.send(json!({ "type": "queue_clear" })).await;
    controller
        .wait_state("an empty queue", |s| {
            s["queue"].as_array().unwrap().is_empty()
        })
        .await;

    // Only acts with music.
    controller
        .send(json!({ "type": "queue_add", "act_id": Uuid::now_v7(), "original": false }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(error["message"], "Für diese Kür gibt es keine Musik.");
}

#[tokio::test]
async fn the_playback_is_public() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let idle = app.get("/api/query/get_playback", None).await;
    assert_eq!(idle.status, StatusCode::OK);
    assert_eq!(idle.body["state"], "idle");
    assert_eq!(idle.body["act_id"], Value::Null);

    // The music control plays in its own browser.
    let act_id = Uuid::now_v7();
    controller
        .send(json!({
            "type": "local_playback",
            "act_id": act_id,
            "original": false,
            "state": "paused",
            "position": 12.5,
            "duration": 180.0,
        }))
        .await;
    let deadline = Instant::now() + TIMEOUT;
    let playback = loop {
        let playback = app.get("/api/query/get_playback", None).await.body;
        if playback["act_id"] == json!(act_id) {
            break playback;
        }
        assert!(Instant::now() < deadline, "no playback within 5 s");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(playback["state"], "paused");
    assert_eq!(playback["position"], 12.5);

    // Closing the music control ends it.
    controller.close().await;
    let deadline = Instant::now() + TIMEOUT;
    while app.get("/api/query/get_playback", None).await.body["state"] != "idle" {
        assert!(
            Instant::now() < deadline,
            "the playback stayed after closing"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn the_controller_removes_a_disconnected_player_at_once() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    let (_buehne, buehne_id) = player(address, &admin, &mut controller, "Bühne").await;
    select(&mut controller, &id).await;

    // A connected player stays.
    controller
        .send(json!({ "type": "remove_player", "player_id": buehne_id }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(
        error["message"],
        "Dieser Player ist verbunden und kann nicht entfernt werden."
    );

    tonpult.close().await;
    controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;
    // Without waiting for the reconnect grace (20 s).
    controller
        .send(json!({ "type": "remove_player", "player_id": id }))
        .await;
    let state = controller
        .wait_state("state without Tonpult", |s| {
            find_player(s, "Tonpult").is_none()
        })
        .await;
    assert_eq!(state["active_player"], Value::Null);
    assert_eq!(connected(&state, "Bühne"), Some(true));
}

#[tokio::test]
async fn switching_players_hands_over_the_waiting_song() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let BertaWaiting {
        address,
        mut controller,
        mut tonpult,
        berta,
        ..
    } = berta_waiting(&app, &admin, "playing").await;
    let (mut laptop, laptop_id) = player(address, &admin, &mut controller, "Laptop").await;

    select(&mut controller, &laptop_id).await;

    // The old player stops, so the new one gets Berta's song right away.
    let message = tonpult
        .wait_for("the stop", |m| m["type"] != "prefetch")
        .await;
    assert_eq!(message, json!({ "type": "stop" }));
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": false })
    );
    assert_eq!(
        laptop.next().await,
        json!({ "type": "active", "active": true })
    );
    assert_eq!(
        laptop.next().await,
        json!({ "type": "volume", "volume": 1.0 })
    );
    assert_eq!(laptop.next().await, cue(original_song(berta, "Berta")));
    let state = serde_json::to_value(app.player_hub.subscribe().0).unwrap();
    assert_eq!(state["cue"], original_song(berta, "Berta"));
    assert_eq!(state["pending_cue"], Value::Null);
}

#[tokio::test]
async fn a_waiting_song_takes_over_when_the_playing_player_leaves() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let BertaWaiting {
        address,
        mut controller,
        tonpult,
        berta,
        ..
    } = berta_waiting(&app, &admin, "playing").await;
    app.player_hub.set_timing(short_grace());

    tonpult.close().await;

    // Its music may go on while it may come back: Berta's song still waits.
    let state = controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    let state = controller
        .wait_state("state without Tonpult", |s| {
            find_player(s, "Tonpult").is_none()
        })
        .await;
    assert_eq!(state["active_player"], Value::Null);
    assert_eq!(state["cue"], original_song(berta, "Berta"));
    assert_eq!(state["pending_cue"], Value::Null);
    // A player selected next gets Berta's song, not Anna's.
    let (mut laptop, laptop_id) = player(address, &admin, &mut controller, "Laptop").await;
    select(&mut controller, &laptop_id).await;
    let message = laptop
        .wait_for("the laptop's cue", |m| m["type"] == "cue")
        .await;
    assert_eq!(message, cue(original_song(berta, "Berta")));
}

#[tokio::test]
async fn the_controller_can_cue_an_act() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let berta = acts[1];
    let normalized = pretend_normalized(&app, berta).await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;
    tonpult.wait_for("Anna's cue", |m| m["type"] == "cue").await;

    // The normalized copy by default, the original on request.
    controller
        .send(json!({ "type": "cue", "act_id": berta, "original": false }))
        .await;
    assert_eq!(
        tonpult.next().await,
        cue(normalized_song(berta, "Berta", &normalized))
    );
    controller
        .send(json!({ "type": "cue", "act_id": berta, "original": true }))
        .await;
    assert_eq!(tonpult.next().await, cue(original_song(berta, "Berta")));
    // The songs coming up have the original now, too.
    let prefetch = tonpult.next().await;
    assert_eq!(prefetch["type"], "prefetch");
    assert_eq!(prefetch["songs"][1], original_song(berta, "Berta"));
    let state = controller
        .wait_state("state with Berta's original", |s| {
            s["cue"]["original"] == true && s["cue"]["act_id"] == berta.to_string()
        })
        .await;
    assert_eq!(state["pending_cue"], Value::Null);
    assert_eq!(state["original_acts"], json!([berta]));

    // An act without music can't be cued.
    controller
        .send(json!({ "type": "cue", "act_id": Uuid::now_v7(), "original": false }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(error["message"], "Für diese Kür gibt es keine Musik.");
}

#[tokio::test]
async fn malformed_controller_messages_keep_the_connection() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;

    for text in ["kein json", r#"{"type":"tanzen"}"#, r#"{"type":"seek"}"#] {
        controller.send_text(text).await;
        let error = controller.next().await;
        assert_eq!(error["type"], "error", "{text}: {error}");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .starts_with("Unbekannter Befehl"),
            "{text}: {error}"
        );
    }

    // The connection still works.
    controller
        .send(json!({ "type": "volume", "volume": 0.3 }))
        .await;
    let state = controller.next().await;
    assert_eq!(state["type"], "state");
    assert_eq!(state["volume"], 0.3);
}

#[tokio::test]
async fn a_paused_song_keeps_the_next_one_waiting() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    // Paused when Anna's act ended: Berta's song waits.
    let BertaWaiting {
        mut controller,
        mut tonpult,
        anna,
        berta,
        ..
    } = berta_waiting(&app, &admin, "paused").await;

    tonpult.send(status("paused", 20.0)).await;
    let state = controller
        .wait_state("state at 20 s", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["position"] == 20.0
        })
        .await;
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    assert_eq!(state["cue"], original_song(anna, "Anna"));

    // The operator stops it: Berta's song comes.
    controller.send(json!({ "type": "stop" })).await;
    let message = tonpult
        .wait_for("the stop", |m| m["type"] != "prefetch")
        .await;
    assert_eq!(message, json!({ "type": "stop" }));
    tonpult.send(status("ready", 0.0)).await;
    assert_eq!(tonpult.next().await, cue(original_song(berta, "Berta")));
}

#[tokio::test]
async fn the_active_player_reconnects_with_its_id() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let BertaWaiting {
        address,
        mut controller,
        tonpult,
        tonpult_id,
        anna,
        berta,
    } = berta_waiting(&app, &admin, "playing").await;

    // The connection is lost; the music may go on.
    tonpult.close().await;
    let state = controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;
    assert_eq!(state["active_player"], tonpult_id);
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    // Commands meanwhile are lost, the volume is kept.
    controller
        .send(json!({ "type": "volume", "volume": 0.5 }))
        .await;
    controller
        .wait_state("state with volume 0.5", |s| s["volume"] == 0.5)
        .await;
    // Switched away from while it's away, it can't be selected again until
    // it's back.
    controller
        .send(json!({ "type": "select_player", "player_id": null }))
        .await;
    controller
        .wait_state("state without an active player", |s| {
            s["active_player"].is_null()
        })
        .await;
    controller
        .send(json!({ "type": "select_player", "player_id": tonpult_id }))
        .await;
    let error = controller
        .wait_for("an error", |m| m["type"] == "error")
        .await;
    assert_eq!(error["message"], "Dieser Player ist nicht mehr verbunden.");

    // Back with its id: still a known player, but no longer active.
    let id = tonpult_id.as_str().unwrap();
    let query = format!("mode=player&name=Tonpult&id={id}");
    let mut tonpult = Ws::connect(address, &query, &admin).await;
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": false })
    );
    assert_eq!(tonpult.next().await["type"], "prefetch");
    let state = controller
        .wait_state("state with Tonpult back", |s| {
            connected(s, "Tonpult") == Some(true)
        })
        .await;
    assert_eq!(state["players"].as_array().unwrap().len(), 1);
    assert_eq!(find_player(&state, "Tonpult").unwrap()["id"], tonpult_id);

    // Back to Anna's judging. Selected again, it plays her song while Berta
    // starts again; then the connection is lost once more and it comes back
    // as the active player.
    let response = app
        .post("/api/command/timeplan_backward", json!(null), Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    controller
        .send(json!({ "type": "cue", "act_id": anna, "original": false }))
        .await;
    select(&mut controller, &tonpult_id).await;
    tonpult
        .wait_for("Anna's cue", |m| m == &cue(original_song(anna, "Anna")))
        .await;
    tonpult.send(status("playing", 10.0)).await;
    controller
        .wait_state("state at 10 s", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["position"] == 10.0
        })
        .await;
    forward(&app, &admin).await;
    controller
        .wait_state("state with Berta pending", |s| !s["pending_cue"].is_null())
        .await;
    tonpult.close().await;
    controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;

    // Active again, with the volume and the song it may still play.
    let mut tonpult = Ws::connect(address, &query, &admin).await;
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "active", "active": true })
    );
    let prefetch = tonpult.next().await;
    assert_eq!(prefetch["type"], "prefetch");
    assert_eq!(act_ids(&prefetch["songs"]), ids(&[berta]));
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "volume", "volume": 0.5 })
    );
    assert_eq!(tonpult.next().await, cue(original_song(anna, "Anna")));
    let state = controller
        .wait_state("state with Tonpult back", |s| {
            connected(s, "Tonpult") == Some(true)
        })
        .await;
    assert_eq!(state["active_player"], tonpult_id);

    // It still plays Anna's song: Berta's waits until it ends.
    tonpult.send(status("playing", 30.0)).await;
    let state = controller
        .wait_state("state at 30 s", |s| {
            find_player(s, "Tonpult").unwrap()["status"]["position"] == 30.0
        })
        .await;
    assert_eq!(state["pending_cue"], original_song(berta, "Berta"));
    tonpult.send(status("ended", 180.0)).await;
    assert_eq!(tonpult.next().await, cue(original_song(berta, "Berta")));
}

#[tokio::test]
async fn players_keep_their_id() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;

    let id = Uuid::now_v7();
    let query = format!("name=Tonpult&id={id}");
    let (mut tonpult, tonpult_id) =
        player_query(address, &admin, &mut controller, &query, "Tonpult").await;
    assert_eq!(tonpult_id, json!(id));
    // An invalid id gets a fresh one.
    let (_laptop, laptop_id) = player_query(
        address,
        &admin,
        &mut controller,
        "name=Laptop&id=kaputt",
        "Laptop",
    )
    .await;
    assert!(Uuid::parse_str(laptop_id.as_str().unwrap()).is_ok());

    // A second connection with the same id takes over, e.g. after a network
    // loss the server didn't notice yet. The first one is closed.
    let query = format!("mode=player&name=Tonpult&id={id}");
    let mut second = Ws::connect(address, &query, &admin).await;
    assert_eq!(
        second.next().await,
        json!({ "type": "active", "active": false })
    );
    assert_eq!(tonpult.closed().await, Some(4000));
    // Give the closed connection time to (not) disconnect the player.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let state = serde_json::to_value(app.player_hub.subscribe().0).unwrap();
    assert_eq!(state["players"].as_array().unwrap().len(), 2);
    assert_eq!(connected(&state, "Tonpult"), Some(true));
}

#[tokio::test]
async fn the_server_pings_and_closes_silent_connections() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    app.player_hub.set_timing(Timing {
        ping_every: Duration::from_millis(100),
        idle_timeout: Duration::from_secs(1),
        ..Timing::default()
    });
    let address = app.serve().await;
    let (mut silent_controller, _) = controller(address, &admin).await;
    let (mut controller, _) = controller(address, &admin).await;
    // Reads its first messages, then nothing: it doesn't answer the pings.
    let (mut tonpult, _) = player(address, &admin, &mut controller, "Tonpult").await;

    let state = controller
        .wait_state("state with Tonpult disconnected", |s| {
            connected(s, "Tonpult") == Some(false)
        })
        .await;
    // It stays for a reconnect.
    assert_eq!(state["players"].as_array().unwrap().len(), 1);
    assert_eq!(tonpult.closed().await, None);
    assert_eq!(silent_controller.closed().await, None);

    // A connection that answers the pings stays open.
    let pings = controller.read_for(Duration::from_millis(1500)).await;
    assert!(pings >= 5, "{pings} pings");
    controller
        .send(json!({ "type": "volume", "volume": 0.5 }))
        .await;
    controller
        .wait_state("state with volume 0.5", |s| s["volume"] == 0.5)
        .await;
}

#[tokio::test]
async fn a_fresh_server_preloads_the_songs_coming_up() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let address = app.serve().await;

    // Before any step in the timeplan.
    for name in ["Tonpult", "Laptop"] {
        let query = format!("mode=player&name={name}");
        let mut player = Ws::connect(address, &query, &admin).await;
        assert_eq!(
            player.next().await,
            json!({ "type": "active", "active": false })
        );
        let prefetch = player.next().await;
        assert_eq!(prefetch["type"], "prefetch");
        assert_eq!(act_ids(&prefetch["songs"]), ids(&acts), "{name}");
    }
}

#[tokio::test]
async fn the_original_choice_is_remembered_per_act() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    let anna_normalized = normalized_song(anna, "Anna", &pretend_normalized(&app, anna).await);
    let berta_normalized = normalized_song(berta, "Berta", &pretend_normalized(&app, berta).await);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;
    let message = tonpult.wait_for("Anna's cue", |m| m["type"] == "cue").await;
    assert_eq!(message, cue(anna_normalized.clone()));

    // Berta's original, chosen ahead; Anna stays cued.
    controller
        .send(json!({ "type": "cue", "act_id": berta, "original": true }))
        .await;
    assert_eq!(tonpult.next().await, cue(original_song(berta, "Berta")));
    let prefetch = tonpult.next().await;
    assert_eq!(
        prefetch["songs"],
        json!([anna_normalized, original_song(berta, "Berta")])
    );
    controller
        .send(json!({ "type": "cue", "act_id": anna, "original": false }))
        .await;
    assert_eq!(tonpult.next().await, cue(anna_normalized.clone()));
    // Only the choice for Berta is remembered.
    controller
        .wait_state("state with Anna cued after Berta", |s| {
            s["cue"] == anna_normalized && s["original_acts"] == json!([berta])
        })
        .await;

    // Warm-up, Anna starts, Anna ends: Berta's original is cued.
    for _ in 0..3 {
        forward(&app, &admin).await;
    }
    let message = tonpult
        .wait_for("Berta's cue", |m| m["type"] == "cue")
        .await;
    assert_eq!(message, cue(original_song(berta, "Berta")));
    let state = controller
        .wait_state("state with Berta cued", |s| {
            s["cue"]["act_id"] == berta.to_string()
        })
        .await;
    assert_eq!(state["cue"], original_song(berta, "Berta"));

    // Back to the normalized copy, also for the songs coming up.
    controller
        .send(json!({ "type": "cue", "act_id": berta, "original": false }))
        .await;
    assert_eq!(tonpult.next().await, cue(berta_normalized.clone()));
    assert_eq!(
        tonpult.next().await,
        json!({ "type": "prefetch", "songs": [berta_normalized] })
    );
    let state = controller
        .wait_state("state without originals", |s| {
            s["original_acts"] == json!([])
        })
        .await;
    assert_eq!(state["cue"], berta_normalized);
}

#[tokio::test]
async fn a_published_final_plays_the_songs_of_its_placed_acts() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let acts = acts_with_songs(&app, &admin, &["Anna", "Berta"]).await;
    let (anna, berta) = (acts[0], acts[1]);
    // The final after the heat, Berta in its first slot, Anna in its second.
    let entry = json!({
        "earliest_start_time": null,
        "duration_seconds": null,
        "label": null,
        "category": null,
        "final_name": "Finale EW",
    });
    let response = app
        .post("/api/command/add_timeplan_entry", entry, Some(&admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let finals = app.get("/api/query/list_finals", Some(&admin)).await.body;
    let slots = &finals[0]["slots"];
    let response = app
        .post(
            "/api/command/set_final_placement",
            json!({
                "name": "Finale EW",
                "slots": [
                    { "slot_id": slots[0]["id"], "act_id": berta },
                    { "slot_id": slots[1]["id"], "act_id": anna },
                ],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let address = app.serve().await;
    let (mut controller, _) = controller(address, &admin).await;
    let (mut tonpult, id) = player(address, &admin, &mut controller, "Tonpult").await;
    select(&mut controller, &id).await;

    let message = tonpult.wait_for("Anna's cue", |m| m["type"] == "cue").await;
    assert_eq!(message, cue(original_song(anna, "Anna")));

    // Once published, the final's songs come up after the heat's.
    let response = app
        .post(
            "/api/command/set_final_published",
            json!({ "name": "Finale EW", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let prefetch = tonpult
        .wait_for("the songs coming up", |m| m["type"] == "prefetch")
        .await;
    assert_eq!(
        act_ids(&prefetch["songs"]),
        ids(&[anna, berta, berta, anna])
    );

    // The heat: warm-up, Anna, Berta. Berta's song comes again for the final.
    for _ in 0..5 {
        forward(&app, &admin).await;
    }
    let message = tonpult
        .wait_for("the final's first cue", |m| {
            m["type"] == "cue" && m["song"]["act_id"] == json!(berta)
        })
        .await;
    assert_eq!(message, cue(original_song(berta, "Berta")));
    let prefetch = tonpult
        .wait_for("the final's songs coming up", |m| m["type"] == "prefetch")
        .await;
    // The heat might still be on its way out; the last list counts.
    let mut songs = act_ids(&prefetch["songs"]);
    while songs != ids(&[berta, anna]) {
        let prefetch = tonpult
            .wait_for("the final's songs coming up", |m| m["type"] == "prefetch")
            .await;
        songs = act_ids(&prefetch["songs"]);
    }

    // The final's warm-up, Berta, then Anna is due.
    forward(&app, &admin).await;
    forward(&app, &admin).await;
    forward(&app, &admin).await;
    let message = tonpult.wait_for("Anna's cue", |m| m["type"] == "cue").await;
    assert_eq!(message, cue(original_song(anna, "Anna")));
}
