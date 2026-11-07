//! `GET /api/ws/player?mode=controller|player&kind=web|cli&name=…&id=…`: the
//! websocket of remote playback. Both modes need the permission to control
//! the music; the sound desk logs in with the Music control role, too.
//!
//! The server pings both modes every [`PING_EVERY`](super::PING_EVERY) and
//! closes a connection that sent nothing for
//! [`IDLE_TIMEOUT`](super::IDLE_TIMEOUT), so a player whose network is gone
//! is noticed without a closing handshake. A player connection that a newer
//! one of the same player (same `id`) replaced is closed with [`REPLACED`].

use std::sync::Arc;

use axum::{
    Extension,
    body::Bytes,
    extract::{
        Query, WebSocketUpgrade,
        ws::{CloseFrame, Message, Utf8Bytes, WebSocket},
    },
    response::Response,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{Duration, Instant, Interval, MissedTickBehavior, interval_at, sleep_until},
};
use tracing::{info, warn};
use uuid::Uuid;

use super::{
    ControllerMessage, PlayerConnection, PlayerHub, PlayerKind, PlayerMessage, Timing,
    ToController, ToPlayer,
};
use crate::{http_server::extractor::auth::Granted, roles::permit};

/// The close code of a player connection that a newer connection of the
/// same player took over, e.g. the same page open twice.
pub const REPLACED: u16 = 4000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Controller,
    Player,
}

/// The players that connect over the websocket; the server player doesn't.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKind {
    #[default]
    Web,
    Cli,
}

impl From<RemoteKind> for PlayerKind {
    fn from(kind: RemoteKind) -> Self {
        match kind {
            RemoteKind::Web => PlayerKind::Web,
            RemoteKind::Cli => PlayerKind::Cli,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PlayerQuery {
    mode: Mode,
    /// Players: the web player page (default) or `cup-player`.
    #[serde(default)]
    kind: RemoteKind,
    /// The player's name, shown in the music control.
    name: Option<String>,
    /// The player's id, a UUID the page keeps across reconnects. A missing
    /// or invalid one gets a fresh id.
    id: Option<String>,
}

pub async fn player_ws(
    upgrade: WebSocketUpgrade,
    _: Granted<permit::ControlMusic>,
    Query(query): Query<PlayerQuery>,
    Extension(hub): Extension<Arc<PlayerHub>>,
) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        match query.mode {
            Mode::Controller => controller(socket, hub).await,
            Mode::Player => {
                let name = query
                    .name
                    .map(|name| name.trim().chars().take(60).collect::<String>())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| match query.kind {
                        RemoteKind::Web => "Web-Player".into(),
                        RemoteKind::Cli => "CLI-Player".into(),
                    });
                let id = query.id.and_then(|id| Uuid::parse_str(id.trim()).ok());
                player(socket, hub, id, name, query.kind.into()).await;
            }
        }
    })
}

fn text<T: Serialize>(message: &T) -> Message {
    Message::Text(Utf8Bytes::from(
        serde_json::to_string(message).expect("messages serialize"),
    ))
}

/// Pings the peer, and notices when it went silent: nothing, not even a
/// pong, for the idle timeout.
struct Heartbeat {
    ping: Interval,
    last_seen: Instant,
    idle_timeout: Duration,
}

enum Beat {
    Ping,
    Silent,
}

impl Heartbeat {
    fn new(timing: Timing) -> Self {
        let now = Instant::now();
        let mut ping = interval_at(now + timing.ping_every, timing.ping_every);
        ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            ping,
            last_seen: now,
            idle_timeout: timing.idle_timeout,
        }
    }

    /// Time to ping, or the peer went silent.
    async fn next(&mut self) -> Beat {
        tokio::select! {
            _ = self.ping.tick() => Beat::Ping,
            () = sleep_until(self.last_seen + self.idle_timeout) => Beat::Silent,
        }
    }

    /// Any message counts.
    fn seen(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Pings; false if the connection is gone or silent.
    async fn handle(&self, beat: Beat, socket: &mut WebSocket) -> bool {
        match beat {
            Beat::Ping => socket.send(Message::Ping(Bytes::new())).await.is_ok(),
            Beat::Silent => {
                info!("Closing a silent playback websocket");
                false
            }
        }
    }
}

async fn controller(socket: WebSocket, hub: Arc<PlayerHub>) {
    let connection = hub.controller_connected();
    run_controller(socket, &hub, connection).await;
    hub.controller_gone(connection);
}

async fn run_controller(mut socket: WebSocket, hub: &PlayerHub, connection: u64) {
    let mut heartbeat = Heartbeat::new(hub.timing());
    let (state, mut updates) = hub.subscribe();
    if socket
        .send(text(&ToController::State(Box::new(state))))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(state) => {
                    if socket.send(text(&ToController::State(Box::new(state)))).await.is_err() {
                        return;
                    }
                }
                // Too slow for the updates: send the latest state instead.
                Err(RecvError::Lagged(_)) => {
                    let (state, fresh) = hub.subscribe();
                    updates = fresh;
                    if socket.send(text(&ToController::State(Box::new(state)))).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Closed) => return,
            },
            beat = heartbeat.next() => {
                if !heartbeat.handle(beat, &mut socket).await {
                    return;
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { return };
                heartbeat.seen();
                let Message::Text(json) = message else { continue };
                let reply = match serde_json::from_str::<ControllerMessage>(&json) {
                    Ok(ControllerMessage::LocalPlayback(playback)) => {
                        hub.local_playback(connection, playback);
                        None
                    }
                    Ok(command) => hub.command(command).await.err(),
                    Err(e) => Some(format!("Unbekannter Befehl: {e}")),
                };
                if let Some(message) = reply
                    && socket.send(text(&ToController::Error { message })).await.is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn player(
    mut socket: WebSocket,
    hub: Arc<PlayerHub>,
    id: Option<Uuid>,
    name: String,
    kind: PlayerKind,
) {
    let mut heartbeat = Heartbeat::new(hub.timing());
    let PlayerConnection {
        id,
        serial,
        mut commands,
    } = hub.register(id, name, kind).await;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    // A newer connection of this player took over.
                    let close = CloseFrame {
                        code: REPLACED,
                        reason: "replaced by a newer connection".into(),
                    };
                    let _ = socket.send(Message::Close(Some(close))).await;
                    break;
                };
                if socket.send(text::<ToPlayer>(&command)).await.is_err() {
                    break;
                }
            }
            beat = heartbeat.next() => {
                if !heartbeat.handle(beat, &mut socket).await {
                    break;
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                heartbeat.seen();
                let Message::Text(json) = message else { continue };
                match serde_json::from_str::<PlayerMessage>(&json) {
                    Ok(PlayerMessage::Status(status)) => hub.status(id, status),
                    Err(e) => warn!("Unknown player message: {e}"),
                }
            }
        }
    }
    hub.disconnected(id, serial);
}
