//! Remote music playback: the music control (a *controller*) drives a
//! *player* on another device over a websocket (`/api/ws/player`), or the
//! server's own sound card (the server player, cargo feature `local-audio`).
//!
//! The [`PlayerHub`] keeps the shared state: the connected players, the
//! active one, the cued song and the volume. Only the active player gets
//! playback commands; every player gets the songs coming up, so it can load
//! them in advance. "Weiter" in the timeplan cues the next song but never
//! starts it: the operator starts the music.
//!
//! A player that loses its connection stays for [`RECONNECT_GRACE`], marked
//! disconnected, and comes back with the same id (`?id=`): it keeps its
//! active status and gets the volume and the cue again. Commands sent while it
//! is away are lost. A controller can remove it before that.
//!
//! For award ceremonies the controllers share a queue of act songs, played on
//! demand: on the active player through the hub, or by a music control in
//! its own browser.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::routes::{
        http_types::ActParticipant,
        query::predict_timeplan::{TimeplanItemStatus, predict},
    },
    normalize::NormalizationStatus,
    reloadable_sqlite::ReloadableSqlite,
    timeplan::{Audience, mark_music_started},
};

#[cfg(feature = "local-audio")]
pub mod audio;
#[cfg(feature = "player-client")]
pub mod client;
#[cfg(feature = "local-audio")]
pub mod local;
pub mod ws;

/// How many upcoming songs the players load in advance.
const PREFETCH: usize = 3;

/// How long a player that lost its connection keeps its place, and its
/// active status, for a reconnect.
pub const RECONNECT_GRACE: Duration = Duration::from_secs(20);
/// How often the server pings a websocket.
pub const PING_EVERY: Duration = Duration::from_secs(10);
/// A websocket that sent nothing for this long, not even a pong, is closed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a music control's report of its own playback counts, as a
/// backstop: a closed connection drops its report at once. Music controls
/// report every second, also from a hidden tab, but browsers slow the timers
/// of a tab that is hidden and silent for minutes to once a minute.
const LOCAL_PLAYBACK_FRESH: Duration = Duration::from_secs(75);

/// The timeouts of remote playback; the tests shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub reconnect_grace: Duration,
    pub ping_every: Duration,
    pub idle_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            reconnect_grace: RECONNECT_GRACE,
            ping_every: PING_EVERY,
            idle_timeout: IDLE_TIMEOUT,
        }
    }
}

/// A song as the players load it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Song {
    pub act_id: Uuid,
    /// The file in the data directory.
    pub file: String,
    /// Where web players load it from, e.g. `/songs/<file>`.
    pub url: String,
    /// The act, e.g. "Sternenstaub – Anna Muster".
    pub label: String,
    /// The original upload instead of the normalized copy.
    pub original: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    /// Nothing cued.
    Idle,
    Loading,
    /// Cued and ready to start.
    Ready,
    Playing,
    Paused,
    /// The song played to its end.
    Ended,
    Error,
}

impl PlaybackState {
    /// Playing, or paused mid-song: the next song from the timeplan waits.
    fn holds_song(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }

    /// Stopped, over or failed: a waiting song may take over. Not while it
    /// is loading, playing or paused.
    fn takes_next(self) -> bool {
        matches!(self, Self::Idle | Self::Ready | Self::Ended | Self::Error)
    }
}

/// What plays: an act's song, its state and position. A music control
/// reports it for its own browser; [`PlayerHub::playback`] tells it the
/// moderation screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Playback {
    /// The act whose song is loaded, if any.
    pub act_id: Option<Uuid>,
    /// The original upload instead of the normalized copy.
    pub original: bool,
    pub state: PlaybackState,
    /// In seconds.
    pub position: f64,
    /// In seconds, once known.
    pub duration: Option<f64>,
}

impl Playback {
    const IDLE: Self = Self {
        act_id: None,
        original: false,
        state: PlaybackState::Idle,
        position: 0.0,
        duration: None,
    };
}

/// What a player reports about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    /// In seconds.
    pub position: f64,
    /// In seconds, once known.
    pub duration: Option<f64>,
    /// 0–1.
    pub volume: f64,
    /// The loaded song's file.
    pub file: Option<String>,
    pub error: Option<String>,
    /// Web players: whether audio was unlocked by a click (autoplay policy).
    pub unlocked: bool,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Idle,
            position: 0.0,
            duration: None,
            volume: 1.0,
            file: None,
            error: None,
            unlocked: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerKind {
    /// The web player page (`/player`).
    Web,
    /// The server's own sound card.
    Server,
    /// The command-line player `cup-player` on another computer.
    Cli,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct PlayerInfo {
    pub id: Uuid,
    pub name: String,
    pub kind: PlayerKind,
    /// False while the connection is lost: the player stays for a while in
    /// case it comes back. Its status is the last one it reported.
    pub connected: bool,
    pub status: PlayerStatus,
}

/// A song in the award ceremony queue.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct QueueEntry {
    pub id: Uuid,
    pub act_id: Uuid,
    /// The original upload instead of the normalized copy.
    pub original: bool,
    /// The act, e.g. "Sternenstaub – Anna Muster".
    pub label: String,
    /// Played already, on a player or in a music control's browser.
    pub played: bool,
}

/// The state every controller sees.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HubState {
    pub players: Vec<PlayerInfo>,
    pub active_player: Option<Uuid>,
    /// The song on the active player.
    pub cue: Option<Song>,
    /// The next song from the timeplan, waiting until the active player stops
    /// playing the current one.
    pub pending_cue: Option<Song>,
    /// 0–1, applied to the active player.
    pub volume: f64,
    /// The acts whose original upload plays instead of the normalized copy,
    /// as cued last by a controller.
    pub original_acts: Vec<Uuid>,
    /// The award ceremony queue, in order.
    pub queue: Vec<QueueEntry>,
}

/// Controller → server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControllerMessage {
    /// `None` plays in the music control's own browser again.
    SelectPlayer {
        player_id: Option<Uuid>,
    },
    /// Also remembers the choice of the original for the act: the timeplan
    /// cues and the songs coming up use it.
    Cue {
        act_id: Uuid,
        original: bool,
    },
    Play,
    Pause,
    /// Stops and rewinds the cued song.
    Stop,
    /// Removes a player that lost its connection, without waiting for it to
    /// come back; if it was the active one, nothing plays remotely. If it
    /// does come back, it's a new player.
    RemovePlayer {
        player_id: Uuid,
    },
    Seek {
        position: f64,
    },
    Volume {
        volume: f64,
    },
    /// What the music control plays in its own browser, for the moderation
    /// screen; sent every second, also while it shows no song. The websocket
    /// hands it to [`PlayerHub::local_playback`] with its connection.
    LocalPlayback(Playback),
    /// Adds an act's song to the end of the award ceremony queue.
    QueueAdd {
        act_id: Uuid,
        original: bool,
    },
    QueueRemove {
        id: Uuid,
    },
    /// Moves an entry to position `to` (0 is the first).
    QueueMove {
        id: Uuid,
        to: usize,
    },
    /// Plays the entry on the active player: loads and starts it, stopping
    /// what plays.
    QueuePlay {
        id: Uuid,
    },
    /// Marks the entry as played, e.g. after the music control played it in
    /// its own browser.
    QueuePlayed {
        id: Uuid,
    },
    QueueClear,
}

/// Server → controller.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToController {
    State(Box<HubState>),
    Error { message: String },
}

/// Player → server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerMessage {
    Status(PlayerStatus),
}

/// Server → player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToPlayer {
    /// Whether this player is the one that plays.
    Active {
        active: bool,
    },
    /// Load the song, ready to start; stops what is playing. Also sent again
    /// after a reconnect: a player that still plays this song keeps playing.
    Cue {
        song: Song,
    },
    /// Unload the song.
    Clear,
    Play,
    Pause,
    Stop,
    Seek {
        position: f64,
    },
    Volume {
        volume: f64,
    },
    /// Songs coming up, to load in advance.
    Prefetch {
        songs: Vec<Song>,
    },
}

/// A player's connection, from [`PlayerHub::register`].
pub struct PlayerConnection {
    pub id: Uuid,
    /// Tells this connection apart from a later one of the same player.
    pub serial: u64,
    /// The player's commands. Ends when a newer connection of the same
    /// player takes over, or when the player is removed.
    pub commands: mpsc::UnboundedReceiver<ToPlayer>,
}

struct PlayerEntry {
    info: PlayerInfo,
    /// Commands sent while the player is disconnected are lost.
    sender: mpsc::UnboundedSender<ToPlayer>,
    /// The serial of the player's latest connection.
    serial: u64,
    /// When the player last reported its status.
    reported_at: std::time::Instant,
}

struct Inner {
    players: Vec<PlayerEntry>,
    active: Option<Uuid>,
    cue: Option<Song>,
    pending_cue: Option<Song>,
    volume: f64,
    /// `None` until worked out for the first player or timeplan step.
    prefetch: Option<Vec<Song>>,
    /// Acts whose original upload plays instead of the normalized copy.
    originals: BTreeSet<Uuid>,
    /// The act whose song the active player was told to play, the song's
    /// position and the time then; see [`PlayerHub::track_music_start`].
    music_start: Option<(Uuid, f64, std::time::Instant)>,
    /// What each music control plays in its own browser, by connection, and
    /// when it said so.
    local_playback: BTreeMap<u64, (Playback, std::time::Instant)>,
    queue: Vec<QueueEntry>,
    next_serial: u64,
    timing: Timing,
}

/// The shared state of remote playback; see the module docs.
pub struct PlayerHub {
    db: ReloadableSqlite,
    inner: Mutex<Inner>,
    controllers: broadcast::Sender<HubState>,
}

impl PlayerHub {
    pub fn new(db: ReloadableSqlite) -> Arc<Self> {
        Arc::new(Self {
            db,
            inner: Mutex::new(Inner {
                players: vec![],
                active: None,
                cue: None,
                pending_cue: None,
                volume: 1.0,
                prefetch: None,
                originals: BTreeSet::new(),
                music_start: None,
                local_playback: BTreeMap::new(),
                queue: vec![],
                next_serial: 0,
                timing: Timing::default(),
            }),
            controllers: broadcast::channel(64).0,
        })
    }

    pub fn timing(&self) -> Timing {
        self.lock().timing
    }

    /// Shorter timeouts, for the tests. They apply to connections opened and
    /// players disconnected after this.
    pub fn set_timing(&self, timing: Timing) {
        self.lock().timing = timing;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn snapshot(inner: &Inner) -> HubState {
        HubState {
            players: inner.players.iter().map(|p| p.info.clone()).collect(),
            active_player: inner.active,
            cue: inner.cue.clone(),
            pending_cue: inner.pending_cue.clone(),
            volume: inner.volume,
            original_acts: inner.originals.iter().copied().collect(),
            queue: inner.queue.clone(),
        }
    }

    fn publish(&self, inner: &Inner) {
        // No controller connected is fine.
        let _ = self.controllers.send(Self::snapshot(inner));
    }

    fn send_active(inner: &Inner, message: ToPlayer) {
        if let Some(player) = inner
            .players
            .iter()
            .find(|p| Some(p.info.id) == inner.active)
        {
            let _ = player.sender.send(message);
        }
    }

    /// Sets the songs coming up and sends them to every player.
    fn set_prefetch(inner: &mut Inner, songs: Vec<Song>) {
        for player in &inner.players {
            let _ = player.sender.send(ToPlayer::Prefetch {
                songs: songs.clone(),
            });
        }
        inner.prefetch = Some(songs);
    }

    /// A number for a new controller connection, for
    /// [`Self::local_playback`] and [`Self::controller_gone`].
    pub fn controller_connected(&self) -> u64 {
        let mut inner = self.lock();
        inner.next_serial += 1;
        inner.next_serial
    }

    /// What the music control on `controller` plays in its own browser.
    pub fn local_playback(&self, controller: u64, playback: Playback) {
        self.lock()
            .local_playback
            .insert(controller, (playback, std::time::Instant::now()));
    }

    /// Forgets what the closed controller connection played.
    pub fn controller_gone(&self, controller: u64) {
        self.lock().local_playback.remove(&controller);
    }

    /// The current state and the updates after it.
    pub fn subscribe(&self) -> (HubState, broadcast::Receiver<HubState>) {
        let inner = self.lock();
        (Self::snapshot(&inner), self.controllers.subscribe())
    }

    /// Adds a player, or connects a known one again: `id` is the player's id
    /// from its last connection. A new player starts inactive. A known one
    /// keeps whether it is active, and its previous connection ends; as the active
    /// player it gets the volume and the cue again. A missing or unknown id,
    /// or one of a player of another kind, gets a fresh id.
    pub async fn register(
        &self,
        id: Option<Uuid>,
        name: String,
        kind: PlayerKind,
    ) -> PlayerConnection {
        self.load_prefetch().await;
        let (sender, commands) = mpsc::unbounded_channel();
        let mut inner = self.lock();
        inner.next_serial += 1;
        let serial = inner.next_serial;
        let known = id.and_then(|id| {
            inner
                .players
                .iter_mut()
                .find(|p| p.info.id == id && p.info.kind == kind)
        });
        let id = match known {
            Some(player) => {
                info!("Player {name:?} ({kind:?}) reconnected");
                player.info.name = name;
                player.info.connected = true;
                // Ends the commands of the previous connection.
                player.sender = sender.clone();
                player.serial = serial;
                player.reported_at = std::time::Instant::now();
                player.info.id
            }
            None => {
                let id = id
                    .filter(|id| !id.is_nil() && !inner.players.iter().any(|p| p.info.id == *id))
                    .unwrap_or_else(Uuid::now_v7);
                info!("Player {name:?} ({kind:?}) connected");
                inner.players.push(PlayerEntry {
                    info: PlayerInfo {
                        id,
                        name,
                        kind,
                        connected: true,
                        status: PlayerStatus::default(),
                    },
                    sender: sender.clone(),
                    serial,
                    reported_at: std::time::Instant::now(),
                });
                id
            }
        };
        let active = inner.active == Some(id);
        let _ = sender.send(ToPlayer::Active { active });
        let _ = sender.send(ToPlayer::Prefetch {
            songs: inner.prefetch.clone().unwrap_or_default(),
        });
        if active {
            // What it missed while away.
            let _ = sender.send(ToPlayer::Volume {
                volume: inner.volume,
            });
            let _ = sender.send(match inner.cue.clone() {
                Some(song) => ToPlayer::Cue { song },
                None => ToPlayer::Clear,
            });
        }
        self.publish(&inner);
        PlayerConnection {
            id,
            serial,
            commands,
        }
    }

    /// The connection `serial` of a player closed: the player stays, marked
    /// disconnected, and is removed if it isn't back within the reconnect
    /// grace. Nothing happens if a newer connection took over already.
    pub fn disconnected(self: &Arc<Self>, id: Uuid, serial: u64) {
        let grace = {
            let mut inner = self.lock();
            let Some(player) = inner
                .players
                .iter_mut()
                .find(|p| p.info.id == id && p.serial == serial)
            else {
                return;
            };
            info!("Player {:?} lost its connection", player.info.name);
            player.info.connected = false;
            self.publish(&inner);
            inner.timing.reconnect_grace
        };
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            if let Some(hub) = hub.upgrade() {
                hub.remove_if_gone(id, serial);
            }
        });
    }

    /// Removes a player that is still disconnected since its connection
    /// `serial` closed.
    fn remove_if_gone(&self, id: Uuid, serial: u64) {
        let mut inner = self.lock();
        let gone = inner
            .players
            .iter()
            .any(|p| p.info.id == id && p.serial == serial && !p.info.connected);
        if gone {
            info!("Player {id} didn't come back");
            Self::remove(&mut inner, id);
            self.publish(&inner);
        }
    }

    /// Removes a player at once, e.g. the server player without a sound
    /// card; if it was the active one, nothing plays remotely.
    pub fn unregister(&self, id: Uuid) {
        let mut inner = self.lock();
        Self::remove(&mut inner, id);
        self.publish(&inner);
    }

    fn remove(inner: &mut Inner, id: Uuid) {
        inner.players.retain(|p| p.info.id != id);
        if inner.active == Some(id) {
            warn!("The active player is gone");
            inner.active = None;
            Self::release_pending_cue(inner);
        }
    }

    /// The active player stopped or left, so a song waiting for the current
    /// one to end becomes the cue; the next selected player loads it.
    fn release_pending_cue(inner: &mut Inner) {
        if let Some(song) = inner.pending_cue.take() {
            inner.cue = Some(song);
        }
    }

    /// Records a player's status. A cue waiting for the song to finish is
    /// sent once the active player stopped: not while it plays, is paused
    /// mid-song or loads.
    pub fn status(&self, id: Uuid, status: PlayerStatus) {
        let mut inner = self.lock();
        let Some(player) = inner.players.iter_mut().find(|p| p.info.id == id) else {
            return;
        };
        let takes_next = status.state.takes_next();
        let was_playing = player.info.status.state == PlaybackState::Playing;
        player.info.status = status;
        player.reported_at = std::time::Instant::now();
        if inner.active == Some(id) {
            self.track_music_start(&mut inner, id, was_playing);
        }
        if inner.active == Some(id)
            && takes_next
            && let Some(song) = inner.pending_cue.take()
        {
            inner.cue = Some(song.clone());
            Self::send_active(&inner, ToPlayer::Cue { song });
        }
        self.publish(&inner);
    }

    /// Records when the active player's music of the cued act started, for
    /// the act timer. A web player reports "playing" as soon as it was told
    /// to, maybe still loading, so the start counts once the position moves,
    /// dated back by how far it moved, but never before "playing" (e.g. after
    /// a seek).
    fn track_music_start(&self, inner: &mut Inner, id: Uuid, was_playing: bool) {
        let Some(status) = inner
            .players
            .iter()
            .find(|p| p.info.id == id)
            .map(|p| &p.info.status)
        else {
            return;
        };
        let cued = inner
            .cue
            .as_ref()
            .filter(|song| status.file.as_ref() == Some(&song.file))
            .map(|song| song.act_id);
        match (status.state == PlaybackState::Playing, cued) {
            (true, Some(act_id)) if !was_playing => {
                inner.music_start = Some((act_id, status.position, std::time::Instant::now()));
            }
            (true, Some(act_id)) => {
                if let Some((started, from, at)) = inner.music_start
                    && started == act_id
                    && status.position > from
                {
                    let seconds_ago = (status.position - from).min(at.elapsed().as_secs_f64());
                    inner.music_start = None;
                    self.music_started(act_id, seconds_ago);
                }
            }
            _ => inner.music_start = None,
        }
    }

    /// Records the music start of `act_id` in the background, as `status`
    /// can't wait for the database.
    fn music_started(&self, act_id: Uuid, seconds_ago: f64) {
        let db = self.db.clone();
        tokio::spawn(async move {
            let db = db.get().await.clone();
            match mark_music_started(&db, act_id, seconds_ago).await {
                Ok(true) => info!("Music of act {act_id} started"),
                Ok(false) => {}
                Err(e) => warn!("Couldn't record the music start of act {act_id}: {e}"),
            }
        });
    }

    /// What plays now, for the moderation screen: the song on the active
    /// player, or else what a music control plays in its own browser if it
    /// said so lately. With several music controls, one that plays goes
    /// first, then one with a song, then the latest report. The position
    /// moves on by the time since the report.
    pub fn playback(&self) -> Playback {
        let inner = self.lock();
        let active = inner
            .active
            .and_then(|id| inner.players.iter().find(|p| p.info.id == id));
        let (mut playback, reported_at) = if let Some(player) = active {
            let status = &player.info.status;
            let cued = inner
                .cue
                .as_ref()
                .filter(|song| status.file.as_ref() == Some(&song.file));
            let playback = Playback {
                act_id: cued.map(|song| song.act_id),
                original: cued.is_some_and(|song| song.original),
                state: status.state,
                position: status.position,
                duration: status.duration,
            };
            (playback, player.reported_at)
        } else if let Some((playback, at)) = inner
            .local_playback
            .values()
            .filter(|(_, at)| at.elapsed() < LOCAL_PLAYBACK_FRESH)
            .max_by_key(|(playback, at)| {
                (
                    playback.state == PlaybackState::Playing,
                    playback.act_id.is_some(),
                    *at,
                )
            })
        {
            (playback.clone(), *at)
        } else {
            return Playback::IDLE;
        };
        if playback.state == PlaybackState::Playing {
            playback.position += reported_at.elapsed().as_secs_f64();
            if let Some(duration) = playback.duration {
                playback.position = playback.position.min(duration);
            }
        }
        playback
    }

    /// Whether any player, or a music control in its own browser (if it
    /// said so lately), is playing right now. The database restore waits for
    /// the music to stop.
    pub fn is_playing(&self) -> bool {
        let inner = self.lock();
        inner
            .players
            .iter()
            .any(|p| p.info.status.state == PlaybackState::Playing)
            || inner.local_playback.values().any(|(playback, at)| {
                playback.state == PlaybackState::Playing && at.elapsed() < LOCAL_PLAYBACK_FRESH
            })
    }

    /// Forgets what the hub knew of the old data after a database restore:
    /// the cue, the songs coming up, the originals' choices and the award
    /// ceremony queue. The players stay connected; they unload the song. The
    /// fresh state goes to the controllers, and the song of the restored
    /// timeplan is cued.
    pub async fn reset_after_restore(&self) {
        {
            let mut inner = self.lock();
            inner.cue = None;
            inner.pending_cue = None;
            inner.originals.clear();
            inner.queue.clear();
            inner.music_start = None;
            inner.local_playback.clear();
            Self::send_active(&inner, ToPlayer::Clear);
            Self::set_prefetch(&mut inner, vec![]);
            self.publish(&inner);
        }
        if let Err(e) = self.cue_from_timeplan().await {
            warn!("Couldn't cue the song after the database restore: {e}");
        }
    }

    /// Whether the active player plays or is paused mid-song, as it reported
    /// last; also while it is disconnected, as the music may go on.
    fn active_holds_song(inner: &Inner) -> bool {
        inner
            .players
            .iter()
            .any(|p| Some(p.info.id) == inner.active && p.info.status.state.holds_song())
    }

    /// Runs a controller's command.
    pub async fn command(&self, message: ControllerMessage) -> Result<(), String> {
        match message {
            ControllerMessage::SelectPlayer { player_id } => {
                {
                    let mut inner = self.lock();
                    if inner.active == player_id {
                        return Ok(());
                    }
                    if let Some(id) = player_id
                        && !inner
                            .players
                            .iter()
                            .any(|p| p.info.id == id && p.info.connected)
                    {
                        return Err("Dieser Player ist nicht mehr verbunden.".into());
                    }
                    // The old player stops, the new one takes over the cue.
                    Self::send_active(&inner, ToPlayer::Stop);
                    Self::send_active(&inner, ToPlayer::Active { active: false });
                    Self::release_pending_cue(&mut inner);
                    inner.active = player_id;
                    Self::send_active(&inner, ToPlayer::Active { active: true });
                    Self::send_active(
                        &inner,
                        ToPlayer::Volume {
                            volume: inner.volume,
                        },
                    );
                    if let Some(song) = inner.cue.clone() {
                        Self::send_active(&inner, ToPlayer::Cue { song });
                    }
                    self.publish(&inner);
                }
                if player_id.is_some() && self.lock().cue.is_none() {
                    self.cue_from_timeplan().await.map_err(|e| e.to_string())?;
                }
            }
            ControllerMessage::Cue { act_id, original } => {
                let song = self
                    .song(act_id, original)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Für diese Kür gibt es keine Musik.")?;
                let mut inner = self.lock();
                if original {
                    inner.originals.insert(act_id);
                } else {
                    inner.originals.remove(&act_id);
                }
                inner.pending_cue = None;
                inner.cue = Some(song.clone());
                Self::send_active(&inner, ToPlayer::Cue { song: song.clone() });
                Self::prefetch_instead(&mut inner, song);
                self.publish(&inner);
            }
            ControllerMessage::RemovePlayer { player_id } => {
                let mut inner = self.lock();
                let Some(player) = inner.players.iter().find(|p| p.info.id == player_id) else {
                    // Gone already.
                    return Ok(());
                };
                if player.info.connected {
                    return Err(
                        "Dieser Player ist verbunden und kann nicht entfernt werden.".into(),
                    );
                }
                info!("Player {:?} removed", player.info.name);
                Self::remove(&mut inner, player_id);
                self.publish(&inner);
            }
            ControllerMessage::QueueAdd { act_id, original } => {
                let song = self
                    .song(act_id, original)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Für diese Kür gibt es keine Musik.")?;
                let mut inner = self.lock();
                inner.queue.push(QueueEntry {
                    id: Uuid::now_v7(),
                    act_id,
                    original,
                    label: song.label,
                    played: false,
                });
                self.publish(&inner);
            }
            ControllerMessage::QueueRemove { id } => {
                let mut inner = self.lock();
                inner.queue.retain(|entry| entry.id != id);
                self.publish(&inner);
            }
            ControllerMessage::QueueMove { id, to } => {
                let mut inner = self.lock();
                if let Some(from) = inner.queue.iter().position(|entry| entry.id == id) {
                    let entry = inner.queue.remove(from);
                    let to = to.min(inner.queue.len());
                    inner.queue.insert(to, entry);
                    self.publish(&inner);
                }
            }
            ControllerMessage::QueuePlay { id } => {
                let Some(entry) = self.lock().queue.iter().find(|e| e.id == id).cloned() else {
                    return Err("Dieser Song ist nicht mehr in der Liste.".into());
                };
                let song = self
                    .song(entry.act_id, entry.original)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Für diese Kür gibt es keine Musik.")?;
                let mut inner = self.lock();
                if inner.active.is_none() {
                    return Err("Kein Player ausgewählt.".into());
                }
                inner.pending_cue = None;
                inner.cue = Some(song.clone());
                Self::send_active(&inner, ToPlayer::Cue { song });
                Self::send_active(&inner, ToPlayer::Play);
                Self::mark_played(&mut inner, id);
                self.publish(&inner);
            }
            ControllerMessage::QueuePlayed { id } => {
                let mut inner = self.lock();
                Self::mark_played(&mut inner, id);
                self.publish(&inner);
            }
            ControllerMessage::QueueClear => {
                let mut inner = self.lock();
                inner.queue.clear();
                self.publish(&inner);
            }
            ControllerMessage::LocalPlayback(_) => {
                return Err("Die eigene Wiedergabe gilt nur mit einer Verbindung".into());
            }
            ControllerMessage::Play => self.forward(ToPlayer::Play),
            ControllerMessage::Pause => self.forward(ToPlayer::Pause),
            ControllerMessage::Stop => self.forward(ToPlayer::Stop),
            ControllerMessage::Seek { position } => {
                self.forward(ToPlayer::Seek {
                    position: position.max(0.0),
                });
            }
            ControllerMessage::Volume { volume } => {
                let volume = volume.clamp(0.0, 1.0);
                let mut inner = self.lock();
                inner.volume = volume;
                Self::send_active(&inner, ToPlayer::Volume { volume });
                self.publish(&inner);
            }
        }
        Ok(())
    }

    fn mark_played(inner: &mut Inner, id: Uuid) {
        if let Some(entry) = inner.queue.iter_mut().find(|entry| entry.id == id) {
            entry.played = true;
        }
    }

    fn forward(&self, message: ToPlayer) {
        let inner = self.lock();
        Self::send_active(&inner, message);
    }

    /// Puts `song` into the songs coming up in place of its act's other
    /// version, e.g. the original instead of the normalized copy.
    fn prefetch_instead(inner: &mut Inner, song: Song) {
        let Some(mut songs) = inner.prefetch.clone() else {
            return;
        };
        let Some(entry) = songs.iter_mut().find(|s| s.act_id == song.act_id) else {
            return;
        };
        if *entry != song {
            *entry = song;
            Self::set_prefetch(inner, songs);
        }
    }

    /// Cues the running act's song, or the next act's, after the timeplan
    /// moved; never starts it. While the active player plays or is paused
    /// mid-song, the new song waits until it is stopped or over. Also sends
    /// the songs coming up.
    pub async fn cue_from_timeplan(&self) -> Result<(), sqlx::Error> {
        self.cue_next(false).await
    }

    /// Like [`Self::cue_from_timeplan`] after an act ended: the active player
    /// stops its music first, so the next song needn't wait.
    pub async fn stop_and_cue_from_timeplan(&self) -> Result<(), sqlx::Error> {
        self.cue_next(true).await
    }

    async fn cue_next(&self, stop: bool) -> Result<(), sqlx::Error> {
        if stop {
            self.forward(ToPlayer::Stop);
        }
        let songs = self.upcoming_songs().await?;
        let next = songs.first().cloned();
        let mut inner = self.lock();
        Self::set_prefetch(&mut inner, songs);
        let same_act = |song: &Option<Song>| {
            song.as_ref().map(|s| s.act_id) == next.as_ref().map(|s| s.act_id)
        };
        if same_act(&inner.cue) {
            inner.pending_cue = None;
        } else if !stop && Self::active_holds_song(&inner) {
            inner.pending_cue = next;
        } else {
            inner.pending_cue = None;
            inner.cue = next.clone();
            match next {
                Some(song) => Self::send_active(&inner, ToPlayer::Cue { song }),
                None => Self::send_active(&inner, ToPlayer::Clear),
            }
        }
        self.publish(&inner);
        Ok(())
    }

    /// Works out the songs coming up unless that happened already, so the
    /// players of a freshly started server load them before the first step
    /// in the timeplan.
    async fn load_prefetch(&self) {
        if self.lock().prefetch.is_some() {
            return;
        }
        match self.upcoming_songs().await {
            Ok(songs) => {
                let mut inner = self.lock();
                // Unless a timeplan step was quicker.
                if inner.prefetch.is_none() {
                    Self::set_prefetch(&mut inner, songs);
                }
            }
            Err(e) => warn!("Couldn't find the songs coming up: {e}"),
        }
    }

    /// The songs of the running act (if any) and the acts after it, as many
    /// as the players load in advance. The original where a controller chose
    /// it, otherwise the normalized copy.
    async fn upcoming_songs(&self) -> Result<Vec<Song>, sqlx::Error> {
        let acts = self.upcoming_acts().await?;
        let originals = self.lock().originals.clone();
        let mut songs = vec![];
        for act_id in acts.iter().take(PREFETCH + 1) {
            if let Some(song) = self.song(*act_id, originals.contains(act_id)).await? {
                songs.push(song);
            }
        }
        Ok(songs)
    }

    /// The running act (if any) and the acts after it, in timeplan order.
    /// Acts placed in a published final play there again.
    async fn upcoming_acts(&self) -> Result<Vec<Uuid>, sqlx::Error> {
        let db = self.db.get().await.clone();
        let Ok(timeplan) = predict(&db, Audience::OrganizerTeam).await else {
            // An incomplete timeplan has no order to follow.
            return Ok(vec![]);
        };
        Ok(timeplan
            .items
            .iter()
            .filter(|item| item.status != TimeplanItemStatus::Ended)
            .flat_map(|item| item.timeplan_entry.songs())
            .filter(|(_, status)| **status != TimeplanItemStatus::Ended)
            .map(|(act_id, _)| act_id)
            .collect())
    }

    /// The song of an act: the normalized copy once it's ready, unless the
    /// original is wanted. `None` without music.
    async fn song(&self, act_id: Uuid, original: bool) -> Result<Option<Song>, sqlx::Error> {
        let db = self.db.get().await.clone();
        let Some(act) = sqlx::query!(
            r#"
            SELECT
                name,
                group_name,
                song_file,
                song_file_normalized,
                normalization_status as "normalization_status: NormalizationStatus",
                participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>"
            FROM view_act WHERE id = ?
            "#,
            act_id
        )
        .fetch_optional(&db)
        .await?
        else {
            return Ok(None);
        };
        let normalized = act
            .song_file_normalized
            .filter(|_| act.normalization_status == Some(NormalizationStatus::Done));
        let (file, original) = match (original, normalized) {
            (false, Some(file)) => (file, false),
            _ => match act.song_file {
                Some(file) => (file, true),
                None => return Ok(None),
            },
        };
        let who = act.group_name.unwrap_or_else(|| {
            act.participants
                .iter()
                .map(|p| format!("{} {}", p.firstname, p.lastname))
                .collect::<Vec<_>>()
                .join(" & ")
        });
        let label = if act.name.is_empty() {
            who
        } else {
            format!("{} – {who}", act.name)
        };
        Ok(Some(Song {
            act_id,
            url: format!("/songs/{file}"),
            file,
            label,
            original,
        }))
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::error::TryRecvError;

    use super::*;

    fn hub(db: sqlx::SqlitePool) -> Arc<PlayerHub> {
        PlayerHub::new(ReloadableSqlite::new(db, String::new()))
    }

    /// Registers a new web player.
    async fn connect(hub: &PlayerHub, name: &str) -> (Uuid, mpsc::UnboundedReceiver<ToPlayer>) {
        let connection = hub.register(None, name.into(), PlayerKind::Web).await;
        (connection.id, connection.commands)
    }

    async fn select(hub: &PlayerHub, id: Uuid) {
        hub.command(ControllerMessage::SelectPlayer {
            player_id: Some(id),
        })
        .await
        .unwrap();
    }

    fn drain(receiver: &mut mpsc::UnboundedReceiver<ToPlayer>) -> Vec<ToPlayer> {
        let mut messages = vec![];
        while let Ok(message) = receiver.try_recv() {
            messages.push(message);
        }
        messages
    }

    /// The first state update that matches; fails after 2 s.
    async fn wait_state(
        updates: &mut broadcast::Receiver<HubState>,
        matches: impl Fn(&HubState) -> bool,
    ) -> HubState {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let state = updates.recv().await.unwrap();
                if matches(&state) {
                    return state;
                }
            }
        })
        .await
        .expect("no matching state within 2 s")
    }

    fn in_state(state: PlaybackState) -> PlayerStatus {
        PlayerStatus {
            state,
            ..PlayerStatus::default()
        }
    }

    fn playing() -> PlayerStatus {
        in_state(PlaybackState::Playing)
    }

    fn song(act: u128) -> Song {
        Song {
            act_id: Uuid::from_u128(act),
            file: format!("{act}.mp3"),
            url: format!("/songs/{act}.mp3"),
            label: "Kür".into(),
            original: false,
        }
    }

    fn short_grace() -> Timing {
        Timing {
            reconnect_grace: Duration::from_millis(50),
            ..Timing::default()
        }
    }

    #[tokio::test]
    async fn only_the_active_player_gets_commands() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        let (_, mut b_rx) = connect(&hub, "B").await;
        drain(&mut a_rx);
        drain(&mut b_rx);

        select(&hub, a).await;
        hub.command(ControllerMessage::Play).await.unwrap();

        let a_messages = drain(&mut a_rx);
        assert!(a_messages.contains(&ToPlayer::Active { active: true }));
        assert!(a_messages.contains(&ToPlayer::Play));
        // B only gets the songs coming up, no playback commands.
        let b_messages = drain(&mut b_rx);
        assert!(
            b_messages
                .iter()
                .all(|m| matches!(m, ToPlayer::Prefetch { .. })),
            "{b_messages:?}"
        );
        assert_eq!(hub.subscribe().0.active_player, Some(a));
    }

    #[tokio::test]
    async fn switching_players_stops_the_old_one() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        let PlayerConnection {
            id: b,
            commands: mut b_rx,
            ..
        } = hub.register(None, "B".into(), PlayerKind::Server).await;
        select(&hub, a).await;
        hub.lock().cue = Some(song(1));
        drain(&mut a_rx);
        drain(&mut b_rx);

        select(&hub, b).await;

        assert_eq!(
            drain(&mut a_rx),
            [ToPlayer::Stop, ToPlayer::Active { active: false }]
        );
        let b_messages = drain(&mut b_rx);
        assert!(b_messages.contains(&ToPlayer::Active { active: true }));
        assert!(b_messages.contains(&ToPlayer::Cue { song: song(1) }));
    }

    #[tokio::test]
    async fn unregistering_the_active_player_clears_it() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, _a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;

        hub.unregister(a);

        let state = hub.subscribe().0;
        assert_eq!(state.active_player, None);
        assert!(state.players.is_empty());
        let unknown = hub
            .command(ControllerMessage::SelectPlayer { player_id: Some(a) })
            .await;
        assert!(unknown.is_err());
    }

    #[tokio::test]
    async fn a_reconnect_keeps_the_active_status() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let first = hub.register(None, "Tonpult".into(), PlayerKind::Web).await;
        let id = first.id;
        select(&hub, id).await;
        hub.command(ControllerMessage::Volume { volume: 0.5 })
            .await
            .unwrap();
        hub.lock().cue = Some(song(1));

        drop(first.commands);
        hub.disconnected(id, first.serial);
        let state = hub.subscribe().0;
        assert_eq!(state.active_player, Some(id));
        assert_eq!(state.players.len(), 1);
        assert!(!state.players[0].connected);
        // Lost, nobody listens.
        hub.command(ControllerMessage::Play).await.unwrap();

        // Back, under a new name: the volume and the cue again.
        let mut second = hub
            .register(Some(id), "Tonpult 2".into(), PlayerKind::Web)
            .await;
        assert_eq!(second.id, id);
        assert_eq!(
            drain(&mut second.commands),
            [
                ToPlayer::Active { active: true },
                ToPlayer::Prefetch { songs: vec![] },
                ToPlayer::Volume { volume: 0.5 },
                ToPlayer::Cue { song: song(1) },
            ]
        );
        let state = hub.subscribe().0;
        assert_eq!(state.active_player, Some(id));
        assert_eq!(state.players.len(), 1);
        assert!(state.players[0].connected);
        assert_eq!(state.players[0].name, "Tonpult 2");

        // Nothing cued any more while it was away: it unloads its song.
        hub.disconnected(id, second.serial);
        hub.lock().cue = None;
        let mut third = hub
            .register(Some(id), "Tonpult 2".into(), PlayerKind::Web)
            .await;
        assert_eq!(drain(&mut third.commands).last(), Some(&ToPlayer::Clear));
    }

    #[tokio::test]
    async fn a_newer_connection_replaces_the_old_one() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let mut first = hub.register(None, "Tonpult".into(), PlayerKind::Web).await;
        select(&hub, first.id).await;

        // E.g. the old connection is dead, but the server didn't notice yet.
        let mut second = hub
            .register(Some(first.id), "Tonpult".into(), PlayerKind::Web)
            .await;

        drain(&mut first.commands);
        assert_eq!(first.commands.try_recv(), Err(TryRecvError::Disconnected));
        // Closing the old connection afterwards changes nothing.
        hub.disconnected(first.id, first.serial);
        let state = hub.subscribe().0;
        assert_eq!(state.players.len(), 1);
        assert!(state.players[0].connected);
        drain(&mut second.commands);
        hub.command(ControllerMessage::Play).await.unwrap();
        assert_eq!(drain(&mut second.commands), [ToPlayer::Play]);
    }

    #[tokio::test]
    async fn players_keep_valid_ids_of_their_kind() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let chosen = Uuid::now_v7();
        let player = hub
            .register(Some(chosen), "A".into(), PlayerKind::Web)
            .await;
        assert_eq!(player.id, chosen);
        let nil = hub
            .register(Some(Uuid::nil()), "B".into(), PlayerKind::Web)
            .await;
        assert_ne!(nil.id, Uuid::nil());

        // A web player can't take over the server player.
        let server = hub
            .register(None, "Server".into(), PlayerKind::Server)
            .await;
        let web = hub
            .register(Some(server.id), "C".into(), PlayerKind::Web)
            .await;
        assert_ne!(web.id, server.id);
        let state = hub.subscribe().0;
        assert_eq!(state.players.len(), 4);
        assert!(state.players.iter().all(|p| p.connected));
    }

    #[tokio::test]
    async fn a_player_that_does_not_come_back_is_removed() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        hub.set_timing(short_grace());
        let a = hub.register(None, "A".into(), PlayerKind::Web).await;
        select(&hub, a.id).await;
        hub.status(a.id, playing());
        hub.lock().cue = Some(song(1));
        hub.lock().pending_cue = Some(song(2));
        let (_, mut updates) = hub.subscribe();

        hub.disconnected(a.id, a.serial);

        // For now it stays; its music may go on, so the next song waits.
        let state = hub.subscribe().0;
        assert_eq!(state.active_player, Some(a.id));
        assert_eq!(state.pending_cue, Some(song(2)));
        assert!(!state.players[0].connected);
        let state = wait_state(&mut updates, |s| s.players.is_empty()).await;
        assert_eq!(state.active_player, None);
        assert_eq!(state.cue, Some(song(2)));
        assert_eq!(state.pending_cue, None);
    }

    #[tokio::test]
    async fn a_player_back_in_time_stays() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        hub.set_timing(short_grace());
        let a = hub.register(None, "A".into(), PlayerKind::Web).await;
        select(&hub, a.id).await;

        hub.disconnected(a.id, a.serial);
        let again = hub.register(Some(a.id), "A".into(), PlayerKind::Web).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let state = hub.subscribe().0;
        assert_eq!(state.active_player, Some(a.id));
        assert!(state.players[0].connected);
        // The next loss starts a new grace period.
        let (_, mut updates) = hub.subscribe();
        hub.disconnected(again.id, again.serial);
        wait_state(&mut updates, |s| s.players.is_empty()).await;
    }

    #[tokio::test]
    async fn a_disconnected_player_can_not_be_selected() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let a = hub.register(None, "A".into(), PlayerKind::Web).await;
        let b = hub.register(None, "B".into(), PlayerKind::Web).await;
        select(&hub, a.id).await;
        hub.disconnected(a.id, a.serial);
        hub.disconnected(b.id, b.serial);

        let selected = hub
            .command(ControllerMessage::SelectPlayer {
                player_id: Some(b.id),
            })
            .await;
        assert_eq!(
            selected,
            Err("Dieser Player ist nicht mehr verbunden.".into())
        );
        // Selecting the active one again changes nothing.
        select(&hub, a.id).await;
        assert_eq!(hub.subscribe().0.active_player, Some(a.id));
    }

    #[tokio::test]
    async fn playback_is_the_active_players_or_else_the_local_one() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        assert_eq!(hub.playback(), Playback::IDLE);

        let local = Playback {
            act_id: Some(Uuid::from_u128(2)),
            original: true,
            state: PlaybackState::Paused,
            position: 12.0,
            duration: Some(180.0),
        };
        let control = hub.controller_connected();
        hub.local_playback(control, local.clone());
        assert_eq!(hub.playback(), local);

        // An active player goes first; its position moves on while it plays.
        let (a, _a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;
        hub.lock().cue = Some(song(1));
        hub.status(
            a,
            PlayerStatus {
                file: Some(song(1).file),
                position: 30.0,
                duration: Some(200.0),
                ..playing()
            },
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        let playback = hub.playback();
        assert_eq!(playback.act_id, Some(Uuid::from_u128(1)));
        assert_eq!(playback.state, PlaybackState::Playing);
        assert!((30.1..31.0).contains(&playback.position), "{playback:?}");
    }

    #[tokio::test]
    async fn the_music_control_that_plays_goes_first() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let report = |state, position| Playback {
            act_id: Some(Uuid::from_u128(2)),
            original: false,
            state,
            position,
            duration: Some(180.0),
        };
        let playing = hub.controller_connected();
        let ready = hub.controller_connected();
        let idle = hub.controller_connected();
        hub.local_playback(playing, report(PlaybackState::Paused, 40.0));
        hub.local_playback(playing, report(PlaybackState::Playing, 40.0));
        // Reports from the other music controls come later.
        hub.local_playback(ready, report(PlaybackState::Ready, 0.0));
        hub.local_playback(idle, Playback::IDLE);
        let playback = hub.playback();
        assert_eq!(playback.state, PlaybackState::Playing);
        assert!((40.0..41.0).contains(&playback.position), "{playback:?}");

        // Closing the music control forgets its report.
        hub.controller_gone(playing);
        assert_eq!(hub.playback(), report(PlaybackState::Ready, 0.0));
        hub.controller_gone(ready);
        assert_eq!(hub.playback(), Playback::IDLE);
    }

    #[tokio::test]
    async fn only_a_disconnected_player_can_be_removed() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let a = hub.register(None, "A".into(), PlayerKind::Web).await;
        let b = hub.register(None, "B".into(), PlayerKind::Web).await;
        select(&hub, a.id).await;
        let remove = |id| ControllerMessage::RemovePlayer { player_id: id };

        assert_eq!(
            hub.command(remove(b.id)).await,
            Err("Dieser Player ist verbunden und kann nicht entfernt werden.".into())
        );
        // The active player, at once instead of after the reconnect grace.
        hub.disconnected(a.id, a.serial);
        hub.command(remove(a.id)).await.unwrap();
        let state = hub.subscribe().0;
        assert_eq!(state.active_player, None);
        let names: Vec<_> = state.players.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["B"]);
        // Removed already: nothing to do.
        hub.command(remove(a.id)).await.unwrap();
        // Back after all: a new, inactive player with the same id.
        let again = hub.register(Some(a.id), "A".into(), PlayerKind::Web).await;
        assert_eq!(again.id, a.id);
        assert_eq!(hub.subscribe().0.active_player, None);
    }

    #[tokio::test]
    async fn a_pending_cue_waits_until_the_song_stops() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;
        hub.status(a, playing());
        hub.lock().pending_cue = Some(song(2));
        drain(&mut a_rx);

        // Still playing: nothing changes.
        hub.status(a, playing());
        assert!(drain(&mut a_rx).is_empty());

        hub.status(a, in_state(PlaybackState::Ended));
        assert_eq!(drain(&mut a_rx), [ToPlayer::Cue { song: song(2) }]);
        let state = hub.subscribe().0;
        assert_eq!(state.cue, Some(song(2)));
        assert_eq!(state.pending_cue, None);
    }

    #[tokio::test]
    async fn only_a_stopped_song_lets_the_next_one_in() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;
        hub.lock().cue = Some(song(1));
        hub.lock().pending_cue = Some(song(2));
        drain(&mut a_rx);

        // E.g. a pause mid-song keeps the paused song.
        for state in [
            PlaybackState::Playing,
            PlaybackState::Paused,
            PlaybackState::Loading,
        ] {
            hub.status(a, in_state(state));
            assert!(drain(&mut a_rx).is_empty(), "{state:?}");
            assert_eq!(hub.subscribe().0.pending_cue, Some(song(2)), "{state:?}");
        }
        for state in [
            PlaybackState::Idle,
            PlaybackState::Ready,
            PlaybackState::Ended,
            PlaybackState::Error,
        ] {
            hub.lock().cue = Some(song(1));
            hub.lock().pending_cue = Some(song(2));
            hub.status(a, in_state(state));
            assert_eq!(
                drain(&mut a_rx),
                [ToPlayer::Cue { song: song(2) }],
                "{state:?}"
            );
            let hub_state = hub.subscribe().0;
            assert_eq!(hub_state.cue, Some(song(2)), "{state:?}");
            assert_eq!(hub_state.pending_cue, None, "{state:?}");
        }
    }

    #[tokio::test]
    async fn a_pending_cue_takes_over_when_the_player_goes() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, _a_rx) = connect(&hub, "A").await;
        let (b, mut b_rx) = connect(&hub, "B").await;
        let (c, mut c_rx) = connect(&hub, "C").await;
        let wait = |player| {
            hub.lock().cue = Some(song(1));
            hub.lock().pending_cue = Some(song(2));
            hub.status(player, playing());
        };

        // Switching players stops the song: the new one loads the next.
        select(&hub, a).await;
        wait(a);
        drain(&mut b_rx);
        select(&hub, b).await;
        assert!(drain(&mut b_rx).contains(&ToPlayer::Cue { song: song(2) }));
        let state = hub.subscribe().0;
        assert_eq!(state.cue, Some(song(2)));
        assert_eq!(state.pending_cue, None);

        // The same when the playing player goes.
        wait(b);
        hub.unregister(b);
        let state = hub.subscribe().0;
        assert_eq!(state.cue, Some(song(2)));
        assert_eq!(state.pending_cue, None);
        drain(&mut c_rx);
        select(&hub, c).await;
        assert!(drain(&mut c_rx).contains(&ToPlayer::Cue { song: song(2) }));
    }

    #[tokio::test]
    async fn ending_an_act_stops_the_music() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;
        hub.lock().cue = Some(song(1));
        hub.status(a, playing());
        drain(&mut a_rx);

        // Any other step: the song plays on.
        hub.cue_from_timeplan().await.unwrap();
        assert!(!drain(&mut a_rx).contains(&ToPlayer::Stop));
        assert_eq!(hub.subscribe().0.cue, Some(song(1)));

        // The next song (none here) comes at once, not once the player says
        // it stopped.
        hub.stop_and_cue_from_timeplan().await.unwrap();
        assert_eq!(
            drain(&mut a_rx),
            [
                ToPlayer::Stop,
                ToPlayer::Prefetch { songs: vec![] },
                ToPlayer::Clear
            ]
        );
        let state = hub.subscribe().0;
        assert_eq!(state.cue, None);
        assert_eq!(state.pending_cue, None);
    }

    #[tokio::test]
    async fn the_first_player_gets_the_songs_coming_up() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        assert_eq!(hub.lock().prefetch, None);

        // Without acts nothing comes up, but that is known now.
        let (_, mut a_rx) = connect(&hub, "A").await;
        assert_eq!(hub.lock().prefetch, Some(vec![]));
        assert_eq!(
            drain(&mut a_rx),
            [
                ToPlayer::Active { active: false },
                ToPlayer::Prefetch { songs: vec![] }
            ]
        );
        // It is worked out once; later players get the list as it is.
        hub.lock().prefetch = Some(vec![song(1)]);
        let (_, mut b_rx) = connect(&hub, "B").await;
        assert_eq!(
            drain(&mut b_rx),
            [
                ToPlayer::Active { active: false },
                ToPlayer::Prefetch {
                    songs: vec![song(1)]
                }
            ]
        );
    }

    #[tokio::test]
    async fn a_cued_version_replaces_the_one_coming_up() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        hub.lock().prefetch = Some(vec![song(1), song(2)]);
        let (_, mut a_rx) = connect(&hub, "A").await;
        drain(&mut a_rx);
        let original = Song {
            file: "2-original.mp3".into(),
            url: "/songs/2-original.mp3".into(),
            original: true,
            ..song(2)
        };

        PlayerHub::prefetch_instead(&mut hub.lock(), original.clone());

        let songs = vec![song(1), original.clone()];
        assert_eq!(hub.lock().prefetch, Some(songs.clone()));
        assert_eq!(drain(&mut a_rx), [ToPlayer::Prefetch { songs }]);
        // The same version again, or an act not coming up: nothing to send.
        PlayerHub::prefetch_instead(&mut hub.lock(), original);
        PlayerHub::prefetch_instead(&mut hub.lock(), song(3));
        assert!(drain(&mut a_rx).is_empty());
    }

    #[tokio::test]
    async fn starting_the_cued_song_records_the_music_start() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let act = Uuid::from_u128(1);
        sqlx::query(
            "INSERT INTO acts (id, name, act_type, started_at) VALUES (?, '', 'single', datetime('now'))",
        )
        .bind(act)
        .execute(&db)
        .await
        .unwrap();
        let hub = hub(db.clone());
        let (a, _a_rx) = connect(&hub, "A").await;
        select(&hub, a).await;
        hub.lock().cue = Some(song(1));
        let music_started = async || -> Option<String> {
            sqlx::query_scalar("SELECT music_started_at FROM acts WHERE id = ?")
                .bind(act)
                .fetch_one(&db)
                .await
                .unwrap()
        };
        let at = |position: f64| PlayerStatus {
            file: Some(song(1).file),
            position,
            ..playing()
        };

        // Told to play, but still loading: no music yet.
        hub.status(a, at(0.0));
        hub.status(a, at(0.0));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(music_started().await, None);

        // The position moves. It moved further than the time since "playing"
        // (like after a seek), so the music started just now.
        hub.status(a, at(90.0));
        tokio::time::timeout(Duration::from_secs(2), async {
            while music_started().await.is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("music start not recorded within 2 s");
        let seconds_ago: f64 = sqlx::query_scalar(
            "SELECT (julianday('now') - julianday(music_started_at)) * 86400 FROM acts WHERE id = ?",
        )
        .bind(act)
        .fetch_one(&db)
        .await
        .unwrap();
        assert!((-1.0..2.0).contains(&seconds_ago), "{seconds_ago}");
    }

    #[tokio::test]
    async fn volume_is_clamped_and_kept() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let hub = hub(db);
        let (a, mut a_rx) = connect(&hub, "A").await;
        hub.command(ControllerMessage::Volume { volume: 3.0 })
            .await
            .unwrap();
        assert_eq!(hub.subscribe().0.volume, 1.0);
        hub.command(ControllerMessage::Volume { volume: 0.5 })
            .await
            .unwrap();
        drain(&mut a_rx);

        // A newly selected player gets the volume.
        select(&hub, a).await;
        assert!(drain(&mut a_rx).contains(&ToPlayer::Volume { volume: 0.5 }));
    }

    #[test]
    fn messages_have_a_type_tag() {
        let message: ControllerMessage =
            serde_json::from_str(r#"{"type":"seek","position":12.5}"#).unwrap();
        assert_eq!(message, ControllerMessage::Seek { position: 12.5 });
        let json = serde_json::to_value(ToPlayer::Play).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "play" }));
    }
}
