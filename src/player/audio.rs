//! Playback on a sound card, shared by the server player ([`super::local`])
//! and the command-line player `cup-player`. Built only with the cargo
//! feature `local-audio`.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use rodio::{
    DeviceSinkBuilder, MixerDeviceSink, Player, Source,
    cpal::{self, traits::HostTrait},
};
use tracing::warn;

use super::{PlaybackState, PlayerStatus, Song, ToPlayer};

/// How often the position is reported while playing.
const REPORT_EVERY: Duration = Duration::from_secs(1);

/// A song's decoded audio.
pub type Audio = Box<dyn Source + Send>;

/// Opens a song's audio, e.g. from the data directory. `Ok(None)` while it
/// isn't there yet: the engine reports the song as loading until it's cued
/// again.
pub type Open = Box<dyn FnMut(&Song) -> Result<Option<Audio>, String> + Send>;

/// The names of the sound cards to play on.
pub fn output_devices() -> Result<Vec<String>, String> {
    let devices = cpal::default_host()
        .output_devices()
        .map_err(|e| e.to_string())?;
    Ok(devices.map(|device| device.to_string()).collect())
}

/// Opens the sound card named `device`, or else the only one whose name
/// contains it (ignoring case); without a name the default one. Returns it
/// with its name.
pub fn open_sink(device: Option<&str>) -> Result<(MixerDeviceSink, String), String> {
    let Some(wanted) = device else {
        let sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        return Ok((sink, "the default sound card".into()));
    };
    let devices: Vec<_> = cpal::default_host()
        .output_devices()
        .map_err(|e| e.to_string())?
        .collect();
    let device = match devices.iter().find(|d| d.to_string() == wanted) {
        Some(device) => device.clone(),
        None => {
            let lower = wanted.to_lowercase();
            let mut matching = devices
                .iter()
                .filter(|d| d.to_string().to_lowercase().contains(&lower));
            match (matching.next(), matching.next()) {
                (Some(device), None) => device.clone(),
                (None, _) => return Err(format!("No sound card is called {wanted:?}.")),
                (Some(_), Some(_)) => {
                    return Err(format!("Several sound cards match {wanted:?}."));
                }
            }
        }
    };
    let name = device.to_string();
    let sink = DeviceSinkBuilder::from_device(device)
        .and_then(|builder| builder.open_sink_or_fallback())
        .map_err(|e| e.to_string())?;
    Ok((sink, name))
}

/// Plays the cued song as the [`ToPlayer`] commands say.
pub struct Engine {
    sink: MixerDeviceSink,
    player: Player,
    open: Open,
    song: Option<Song>,
    state: PlaybackState,
    duration: Option<f64>,
    volume: f32,
    error: Option<String>,
    /// The status reported last, to report changes right away.
    reported: Option<PlayerStatus>,
}

impl Engine {
    pub fn new(sink: MixerDeviceSink, open: Open) -> Self {
        let player = Player::connect_new(sink.mixer());
        Self {
            sink,
            player,
            open,
            song: None,
            state: PlaybackState::Idle,
            duration: None,
            volume: 1.0,
            error: None,
            reported: None,
        }
    }

    /// Loads the song into a new, paused player at the start.
    fn load(&mut self, song: Song) {
        self.player = Player::connect_new(self.sink.mixer());
        self.player.pause();
        self.player.set_volume(self.volume);
        self.error = None;
        self.duration = None;
        match (self.open)(&song) {
            Ok(Some(audio)) => {
                self.duration = audio.total_duration().map(|d| d.as_secs_f64());
                self.player.append(audio);
                self.state = PlaybackState::Ready;
            }
            Ok(None) => self.state = PlaybackState::Loading,
            Err(e) => {
                warn!("Can't load {}: {e}", song.file);
                self.error = Some(e);
                self.state = PlaybackState::Error;
            }
        }
        self.song = Some(song);
    }

    /// Loads the song again if it played to its end or was stopped.
    fn reload_if_empty(&mut self) {
        if self.player.empty()
            && let Some(song) = self.song.clone()
        {
            self.load(song);
        }
    }

    pub fn handle(&mut self, command: ToPlayer) {
        match command {
            ToPlayer::Cue { song } => {
                // The cue again after a reconnect: the song plays on.
                let same = self.song.as_ref().is_some_and(|s| s.file == song.file);
                if same && matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
                    self.song = Some(song);
                } else {
                    self.load(song);
                }
            }
            ToPlayer::Clear => {
                self.player = Player::connect_new(self.sink.mixer());
                self.song = None;
                self.duration = None;
                self.error = None;
                self.state = PlaybackState::Idle;
            }
            ToPlayer::Play => {
                if self.song.is_some() {
                    self.reload_if_empty();
                    if !matches!(self.state, PlaybackState::Error | PlaybackState::Loading) {
                        self.player.play();
                        self.state = PlaybackState::Playing;
                    }
                }
            }
            ToPlayer::Pause => {
                if self.state == PlaybackState::Playing {
                    self.player.pause();
                    self.state = PlaybackState::Paused;
                }
            }
            ToPlayer::Stop => {
                if let Some(song) = self.song.clone() {
                    self.load(song);
                }
            }
            ToPlayer::Seek { position } => {
                self.reload_if_empty();
                if let Err(e) = self.player.try_seek(Duration::from_secs_f64(position)) {
                    warn!("Can't seek: {e}");
                }
            }
            ToPlayer::Volume { volume } => {
                self.volume = volume.clamp(0.0, 1.0) as f32;
                self.player.set_volume(self.volume);
            }
            // Loading the songs coming up and the active status are up to the
            // player around the engine.
            ToPlayer::Prefetch { .. } | ToPlayer::Active { .. } => {}
        }
    }

    /// Notices the end of the song; true if the status changed.
    pub fn tick(&mut self) -> bool {
        if self.state == PlaybackState::Playing && self.player.empty() {
            self.state = PlaybackState::Ended;
        }
        let status = self.status();
        let changed = self.reported.as_ref().is_none_or(|reported| {
            reported.state != status.state
                || reported.file != status.file
                || reported.volume != status.volume
        });
        if changed {
            self.reported = Some(status);
        }
        changed
    }

    pub fn status(&self) -> PlayerStatus {
        PlayerStatus {
            state: self.state,
            position: self.player.get_pos().as_secs_f64(),
            duration: self.duration,
            volume: f64::from(self.volume),
            file: self.song.as_ref().map(|song| song.file.clone()),
            error: self.error.clone(),
            unlocked: true,
        }
    }
}

/// Runs the engine on this thread until `commands` closes. Every command is
/// answered with a status (e.g. a seek), changes are reported at once and
/// the position every second while playing.
pub fn run(
    mut engine: Engine,
    commands: mpsc::Receiver<ToPlayer>,
    mut report: impl FnMut(PlayerStatus),
) {
    let mut last_report = Instant::now();
    loop {
        let mut handled = false;
        match commands.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => {
                engine.handle(command);
                handled = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let changed = engine.tick();
        if changed
            || handled
            || (engine.state == PlaybackState::Playing && last_report.elapsed() >= REPORT_EVERY)
        {
            report(engine.status());
            last_report = Instant::now();
        }
    }
}
