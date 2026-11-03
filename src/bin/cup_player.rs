//! `cup-player`: plays the acts' music on the sound card of the computer it
//! runs on, driven by the music control like the web player at `/player`.
//! Build it with `cargo build --release --features cli-player --bin
//! cup-player` (needs the ALSA headers, `libasound2-dev` on Linux).

use std::{io::Cursor, path::PathBuf, sync::mpsc};

use clap::Parser;
use eyre::eyre;
use einrad_server::player::{
    Song, ToPlayer,
    audio::{self, Audio, Engine, Open},
    client::{self, Options, Songs},
};
use rodio::Decoder;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use url::Url;
use uuid::Uuid;

/// Plays the acts' music on this computer's sound card, driven by the music
/// control of the registration server.
#[derive(Debug, Parser)]
#[command(name = "cup-player")]
struct Args {
    /// The registration server, e.g. https://anmeldung.example.org or
    /// http://192.168.1.10:3000.
    #[clap(long, env = "CUP_SERVER", required_unless_present = "list_devices")]
    server: Option<Url>,
    /// Email of an admin login.
    #[clap(long, env = "CUP_EMAIL", required_unless_present = "list_devices")]
    email: Option<String>,
    /// Password of the admin login; better set in the environment or in a
    /// .env file than on the command line.
    #[clap(
        long,
        env = "CUP_PASSWORD",
        hide_env_values = true,
        required_unless_present = "list_devices"
    )]
    password: Option<String>,
    /// The name shown in the music control; defaults to this computer's
    /// name. A player with the same name takes over from this one.
    #[clap(long, env = "CUP_PLAYER_NAME")]
    name: Option<String>,
    /// The sound card to play on, by (part of) its name; see
    /// --list-devices. Defaults to the system's default.
    #[clap(long, env = "CUP_DEVICE")]
    device: Option<String>,
    /// Lists the sound cards and exits.
    #[clap(long)]
    list_devices: bool,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    dotenvy::dotenv().ok();
    let args = Args::parse();
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .init();

    if args.list_devices {
        for device in audio::output_devices().map_err(|e| eyre!(e))? {
            println!("{device}");
        }
        return Ok(());
    }
    let (Some(server), Some(email), Some(password)) = (args.server, args.email, args.password)
    else {
        unreachable!("clap requires them without --list-devices");
    };
    let name = args.name.unwrap_or_else(computer_name);
    // The same id for the same name: a restarted player gets its active status
    // back if it's quick.
    let id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("cup-player:{name}").as_bytes(),
    );

    let songs = Songs::default();
    let (commands, commands_rx) = mpsc::channel::<ToPlayer>();
    let (statuses_tx, statuses) = tokio_mpsc::unbounded_channel();
    let (opened_tx, opened) = oneshot::channel();
    // The sound card can't move between threads.
    let thread_songs = songs.clone();
    std::thread::Builder::new()
        .name("cup-player-audio".into())
        .spawn(move || {
            let sink = match audio::open_sink(args.device.as_deref()) {
                Ok((sink, name)) => {
                    let _ = opened_tx.send(Ok(name));
                    sink
                }
                Err(e) => {
                    let _ = opened_tx.send(Err(e));
                    return;
                }
            };
            let open: Open = Box::new(move |song: &Song| {
                let Some(audio) = thread_songs.get(&song.file)? else {
                    return Ok(None);
                };
                let decoder = Decoder::builder()
                    .with_byte_len(audio.len() as u64)
                    .with_seekable(true)
                    .with_data(Cursor::new(audio))
                    .build()
                    .map_err(|e| e.to_string())?;
                Ok(Some(Box::new(decoder) as Audio))
            });
            audio::run(Engine::new(sink, open), commands_rx, |status| {
                let _ = statuses_tx.send(status);
            });
        })?;
    let device = opened
        .await?
        .map_err(|e| eyre!("Can't open the sound card: {e}"))?;
    info!("Plays on {device}");

    let options = Options {
        server,
        email,
        password,
        name,
        id,
    };
    tokio::select! {
        result = client::run(options, songs, commands, statuses) => result.map_err(|e| eyre!(e)),
        _ = tokio::signal::ctrl_c() => {
            info!("Stopped");
            Ok(())
        }
    }
}

/// The computer's name, or "CLI-Player".
fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string(PathBuf::from("/etc/hostname")).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "CLI-Player".into())
}
