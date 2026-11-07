//! The server player: plays on the server's own sound card, for an event
//! server at the sound desk. Built only with the cargo feature `local-audio`
//! and started with the `local_player` setting.

use std::{
    fs::File,
    path::PathBuf,
    sync::{Arc, mpsc},
};

use rodio::Decoder;
use tracing::{error, info};
use uuid::Uuid;

use super::{
    PlayerHub, PlayerKind, Song, ToPlayer,
    audio::{self, Audio, Engine, Open},
};

/// Starts the server player in its own thread. The audio device can't move
/// between threads, so commands reach it through a channel. It registers
/// with one id for its lifetime and never reconnects.
pub fn start(hub: Arc<PlayerHub>, data_path: PathBuf, name: String) {
    let id = Uuid::now_v7();
    tokio::spawn(async move {
        let mut connection = hub
            .register(Some(id), name.clone(), PlayerKind::Server)
            .await;
        let (commands_tx, commands_rx) = mpsc::channel::<ToPlayer>();
        let thread_hub = hub.clone();
        let spawned = std::thread::Builder::new()
            .name("server-player".into())
            .spawn(move || {
                let sink = match audio::open_sink(None) {
                    Ok((sink, _)) => sink,
                    Err(e) => {
                        error!("The server player can't open the sound card: {e}");
                        thread_hub.unregister(id);
                        return;
                    }
                };
                info!("Server player {name:?} plays on the default sound card");
                let open: Open = Box::new(move |song: &Song| {
                    let path = data_path.join(&song.file);
                    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    let decoder = Decoder::try_from(file).map_err(|e| e.to_string())?;
                    Ok(Some(Box::new(decoder) as Audio))
                });
                audio::run(Engine::new(sink, open), commands_rx, |status| {
                    thread_hub.status(id, status);
                });
                thread_hub.unregister(id);
            });
        if let Err(e) = spawned {
            error!("Couldn't start the server player: {e}");
            hub.unregister(id);
            return;
        }
        while let Some(command) = connection.commands.recv().await {
            if commands_tx.send(command).is_err() {
                break;
            }
        }
    });
}
