//! The network side of the command-line player `cup-player`: logs in as an
//! admin, connects to `/api/ws/player` as a player of kind `cli`, downloads
//! the songs into memory and passes the commands on to the sound card (the
//! `output`), reporting its status back. A lost connection is opened again
//! with the same id while the music plays on, so the player keeps its
//! active status. Built with the cargo feature `player-client`.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, mpsc as std_mpsc},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use reqwest::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use tokio::{
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{self, Message, client::IntoClientRequest, http::HeaderValue},
};
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

use super::{PlaybackState, PlayerMessage, PlayerStatus, Song, ToPlayer, ws::REPLACED};

/// A connection that sent nothing for this long, not even one of the
/// server's pings, is given up.
const SILENCE: Duration = super::IDLE_TIMEOUT;
/// The longest wait before connecting again.
const MAX_RETRY: Duration = Duration::from_secs(10);
/// How long connecting and logging in may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How the player connects.
#[derive(Debug, Clone)]
pub struct Options {
    /// The registration server, e.g. `https://anmeldung.example.org`.
    pub server: Url,
    /// An admin login.
    pub email: String,
    pub password: String,
    /// Shown in the music control.
    pub name: String,
    /// The same for every connection, so a reconnect keeps the active status.
    pub id: Uuid,
}

/// The songs in memory, by file: the cued one and the ones coming up. A
/// failed download keeps its error until the song is cued again.
#[derive(Clone, Default)]
pub struct Songs(Arc<Mutex<SongsInner>>);

#[derive(Default)]
struct SongsInner {
    audio: HashMap<String, Result<Arc<[u8]>, String>>,
    cued: Option<String>,
}

impl Songs {
    fn lock(&self) -> MutexGuard<'_, SongsInner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A song's audio: `Ok(None)` while it isn't downloaded yet.
    pub fn get(&self, file: &str) -> Result<Option<Arc<[u8]>>, String> {
        match self.lock().audio.get(file) {
            None => Ok(None),
            Some(Ok(audio)) => Ok(Some(audio.clone())),
            Some(Err(e)) => Err(e.clone()),
        }
    }

    fn has(&self, file: &str) -> bool {
        matches!(self.lock().audio.get(file), Some(Ok(_)))
    }

    fn insert(&self, file: &str, audio: Result<Arc<[u8]>, String>) {
        self.lock().audio.insert(file.to_owned(), audio);
    }

    /// Notes the cued song, which stays in memory; a failed download of it is
    /// tried again.
    fn cue(&self, file: Option<&str>) {
        let mut inner = self.lock();
        if let Some(file) = file
            && matches!(inner.audio.get(file), Some(Err(_)))
        {
            inner.audio.remove(file);
        }
        inner.cued = file.map(str::to_owned);
    }

    /// Forgets the songs that are neither cued nor in `files`.
    fn keep_only(&self, files: &HashSet<&str>) {
        let mut inner = self.lock();
        let cued = inner.cued.clone();
        inner
            .audio
            .retain(|file, _| files.contains(file.as_str()) || cued.as_deref() == Some(file));
    }
}

/// The login and the downloads.
struct Session {
    http: reqwest::Client,
    options: Options,
    /// The `Cookie` header with the login.
    cookie: Mutex<String>,
    /// One download at a time, so the cued song and the ones coming up
    /// aren't downloaded twice.
    downloading: tokio::sync::Mutex<()>,
}

enum LoginError {
    /// Wrong email or password, or no admin.
    Rejected(String),
    /// E.g. the server is unreachable: worth trying again.
    Failed(String),
}

impl Session {
    fn cookie(&self) -> String {
        self.cookie
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    async fn login(&self) -> Result<(), LoginError> {
        let url = self
            .options
            .server
            .join("/api/command/login")
            .map_err(|e| LoginError::Rejected(e.to_string()))?;
        let body = serde_json::json!({
            "email": self.options.email,
            "password": self.options.password,
        });
        let response = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| LoginError::Failed(format!("Login failed: {e}")))?;
        let status = response.status();
        if status.is_client_error() {
            return Err(LoginError::Rejected(format!(
                "The server rejected the login ({status}): wrong email or password?"
            )));
        }
        if !status.is_success() {
            return Err(LoginError::Failed(format!("Login failed: {status}")));
        }
        let jwt = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| value.split(';').next()?.trim().strip_prefix("jwt="))
            .ok_or_else(|| LoginError::Failed("The login set no cookie.".into()))?;
        *self.cookie.lock().unwrap_or_else(|e| e.into_inner()) = format!("jwt={jwt}");
        Ok(())
    }

    /// Downloads a song into `songs` unless it's there.
    async fn fetch(&self, songs: &Songs, song: &Song) -> Result<(), String> {
        let _one_at_a_time = self.downloading.lock().await;
        if songs.has(&song.file) {
            return Ok(());
        }
        let url = self
            .options
            .server
            .join(&song.url)
            .map_err(|e| e.to_string())?;
        info!("Downloading {}", song.label);
        let response = self
            .http
            .get(url)
            .header(COOKIE, self.cookie())
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("Download of {} failed: {e}", song.file))?;
        let audio = response
            .bytes()
            .await
            .map_err(|e| format!("Download of {} failed: {e}", song.file))?;
        songs.insert(&song.file, Ok(Arc::from(audio.as_ref())));
        Ok(())
    }

    /// Opens the websocket.
    async fn connect(&self) -> Result<Socket, ConnectError> {
        let options = &self.options;
        let mut url = options
            .server
            .join("/api/ws/player")
            .map_err(|e| ConnectError::Failed(e.to_string()))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|()| ConnectError::Failed(format!("Can't connect to {url}")))?;
        url.query_pairs_mut()
            .append_pair("mode", "player")
            .append_pair("kind", "cli")
            .append_pair("name", &options.name)
            .append_pair("id", &options.id.to_string());
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|e| ConnectError::Failed(e.to_string()))?;
        let cookie = HeaderValue::from_str(&self.cookie())
            .map_err(|e| ConnectError::Failed(e.to_string()))?;
        request.headers_mut().insert(COOKIE, cookie);
        match timeout(CONNECT_TIMEOUT, connect_async(request)).await {
            Ok(Ok((socket, _))) => Ok(socket),
            Ok(Err(tungstenite::Error::Http(response)))
                if matches!(response.status().as_u16(), 401 | 403) =>
            {
                Err(ConnectError::Unauthorized)
            }
            Ok(Err(e)) => Err(ConnectError::Failed(e.to_string())),
            Err(_) => Err(ConnectError::Failed("Connecting timed out.".into())),
        }
    }
}

enum ConnectError {
    /// The login expired or its user is no admin (anymore).
    Unauthorized,
    Failed(String),
}

/// Why a connection ended.
enum Ended {
    Lost(String),
    /// Another `cup-player` with the same id took over.
    Replaced,
    /// The sound card's thread stopped.
    OutputGone,
}

/// Runs the player: logs in, connects and serves the connection, and
/// connects again whenever it's lost. Returns an error if the login is
/// rejected, another player with the same id takes over or `output` stops.
///
/// `output` gets the commands in order. A cue of a song that isn't
/// downloaded yet comes twice: at once, so the output stops and reports it as
/// loading ([`Songs::get`] is `Ok(None)`), and again once it's in `songs`.
/// `statuses` are the output's reports, all of them sent to the server.
pub async fn run(
    options: Options,
    songs: Songs,
    output: std_mpsc::Sender<ToPlayer>,
    mut statuses: mpsc::UnboundedReceiver<PlayerStatus>,
) -> Result<(), String> {
    // reqwest and the websocket use rustls with ring; the first call wins.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let session = Arc::new(Session {
        http,
        options,
        cookie: Mutex::new(String::new()),
        downloading: tokio::sync::Mutex::new(()),
    });
    let server = session.options.server.clone();

    let (commands, queue) = mpsc::unbounded_channel();
    tokio::spawn(sequence(session.clone(), songs.clone(), output, queue));
    let mut prefetch: Option<JoinHandle<()>> = None;
    let mut last_status: Option<PlayerStatus> = None;
    let mut retry = Duration::from_secs(1);
    let mut logged_in = false;
    loop {
        if !logged_in {
            match session.login().await {
                Ok(()) => logged_in = true,
                Err(LoginError::Rejected(e)) => return Err(e),
                Err(LoginError::Failed(e)) => warn!("{e}"),
            }
        }
        if logged_in {
            match session.connect().await {
                Ok(socket) => {
                    info!("Connected to {server} as {:?}", session.options.name);
                    retry = Duration::from_secs(1);
                    let ended = serve(
                        socket,
                        &session,
                        &songs,
                        &commands,
                        &mut prefetch,
                        &mut statuses,
                        &mut last_status,
                    )
                    .await;
                    match ended {
                        Ended::Lost(reason) => warn!("Connection lost: {reason}"),
                        Ended::Replaced => {
                            return Err(format!(
                                "Another player named {:?} took over.",
                                session.options.name
                            ));
                        }
                        Ended::OutputGone => return Err("The sound card stopped.".into()),
                    }
                }
                Err(ConnectError::Unauthorized) => {
                    warn!("The server wants a new login");
                    logged_in = false;
                    continue;
                }
                Err(ConnectError::Failed(e)) => warn!("Can't connect to {server}: {e}"),
            }
        }
        info!("Trying again in {} s", retry.as_secs());
        sleep(retry).await;
        retry = (retry * 2).min(MAX_RETRY);
    }
}

/// Serves one connection until it ends.
async fn serve(
    mut socket: Socket,
    session: &Arc<Session>,
    songs: &Songs,
    commands: &mpsc::UnboundedSender<ToPlayer>,
    prefetch: &mut Option<JoinHandle<()>>,
    statuses: &mut mpsc::UnboundedReceiver<PlayerStatus>,
    last_status: &mut Option<PlayerStatus>,
) -> Ended {
    // What the server missed meanwhile: only the latest status matters.
    while let Ok(status) = statuses.try_recv() {
        *last_status = Some(status);
    }
    if let Some(status) = last_status.clone()
        && let Err(e) = send_status(&mut socket, status).await
    {
        return Ended::Lost(e);
    }
    loop {
        tokio::select! {
            message = timeout(SILENCE, socket.next()) => {
                let message = match message {
                    Err(_) => return Ended::Lost("the server went silent".into()),
                    Ok(None) => return Ended::Lost("closed".into()),
                    Ok(Some(Err(e))) => return Ended::Lost(e.to_string()),
                    Ok(Some(Ok(message))) => message,
                };
                match message {
                    Message::Text(json) => match serde_json::from_str::<ToPlayer>(&json) {
                        Ok(ToPlayer::Active { active }) => {
                            info!("{}", if active { "Active: this player plays" } else { "Not active" });
                        }
                        Ok(ToPlayer::Prefetch { songs: upcoming }) => {
                            if let Some(task) = prefetch.take() {
                                task.abort();
                            }
                            let files: HashSet<&str> =
                                upcoming.iter().map(|song| song.file.as_str()).collect();
                            songs.keep_only(&files);
                            let (session, songs) = (session.clone(), songs.clone());
                            *prefetch = Some(tokio::spawn(async move {
                                for song in upcoming {
                                    if let Err(e) = session.fetch(&songs, &song).await {
                                        warn!("{e}");
                                    }
                                }
                            }));
                        }
                        Ok(command) => {
                            if let ToPlayer::Cue { song } = &command {
                                info!("Cued {}", song.label);
                            }
                            if commands.send(command).is_err() {
                                return Ended::OutputGone;
                            }
                        }
                        Err(e) => warn!("Unknown command: {e}"),
                    },
                    Message::Close(frame) => {
                        if frame.is_some_and(|frame| u16::from(frame.code) == REPLACED) {
                            return Ended::Replaced;
                        }
                        return Ended::Lost("closed by the server".into());
                    }
                    // tungstenite answers the pings.
                    _ => {}
                }
            }
            status = statuses.recv() => {
                let Some(status) = status else {
                    return Ended::OutputGone;
                };
                if last_status.as_ref().is_none_or(|last| last.state != status.state) {
                    log_state(&status);
                }
                *last_status = Some(status.clone());
                if let Err(e) = send_status(&mut socket, status).await {
                    return Ended::Lost(e);
                }
            }
        }
    }
}

async fn send_status(socket: &mut Socket, status: PlayerStatus) -> Result<(), String> {
    let json = serde_json::to_string(&PlayerMessage::Status(status)).expect("messages serialize");
    socket
        .send(Message::text(json))
        .await
        .map_err(|e| e.to_string())
}

fn log_state(status: &PlayerStatus) {
    let file = status.file.as_deref().unwrap_or("–");
    match (&status.state, &status.error) {
        (PlaybackState::Error, Some(error)) => warn!("Error with {file}: {error}"),
        (state, _) => info!("{state:?}: {file}"),
    }
}

/// Passes the commands on to the output in order. A cue whose song isn't
/// downloaded yet goes out at once (the output reports it as loading) and
/// again once the song is here; the commands after it wait for that.
async fn sequence(
    session: Arc<Session>,
    songs: Songs,
    output: std_mpsc::Sender<ToPlayer>,
    mut queue: mpsc::UnboundedReceiver<ToPlayer>,
) {
    while let Some(command) = queue.recv().await {
        match &command {
            ToPlayer::Cue { song } => {
                songs.cue(Some(&song.file));
                if !songs.has(&song.file) {
                    if output.send(command.clone()).is_err() {
                        return;
                    }
                    if let Err(e) = session.fetch(&songs, song).await {
                        warn!("{e}");
                        songs.insert(&song.file, Err(e));
                    }
                }
            }
            ToPlayer::Clear => songs.cue(None),
            _ => {}
        }
        if output.send(command).is_err() {
            return;
        }
    }
}
