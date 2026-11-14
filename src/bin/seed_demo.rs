//! Recreates the database and fills it with demo data (see `demo-data.toml`).
//!
//! The database and data path come from the same CLI flags, environment
//! variables and config file as the server. The existing database is deleted.

use std::{path::PathBuf, str::FromStr};

use clap::Parser;
use einrad_server::{
    config::{ConfigFile, DEFAULT_DATA_PATH, DEFAULT_DATABASE_URL},
    demo::DemoData,
};
use eyre::eyre;
use sqlx::{migrate, sqlite::SqliteConnectOptions};
use tracing::{info, warn};
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, clap::Parser)]
struct Args {
    /// Demo data file.
    #[clap(long, env = "DEMO_DATA", default_value = "./demo-data.toml")]
    demo: PathBuf,
    /// Where the downloaded songs are kept. Defaults to `demo-songs` next to
    /// the demo data file.
    #[clap(long, env = "DEMO_SONGS")]
    songs: Option<PathBuf>,
    /// Path to the TOML config file. Defaults to `./config.toml` if it exists.
    #[clap(long, env = "CONFIG")]
    config: Option<PathBuf>,
    #[clap(long, env = "DATABASE_URL")]
    db: Option<String>,
    #[clap(long, env = "DATA_PATH")]
    data: Option<PathBuf>,
}

/// A plain `main`: the local UTC offset (for backup names) can only be read
/// before any thread exists, so it is read here, then the runtime is built.
fn main() -> eyre::Result<()> {
    let offset = einrad_server::db_backup::init_local_offset();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(offset.is_ok()))
}

async fn run(local_offset_known: bool) -> eyre::Result<()> {
    color_eyre::install()?;
    dotenvy::dotenv().ok();
    let args = Args::parse();
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .init();
    if !local_offset_known {
        tracing::warn!("Couldn't determine the local time zone: backups are named in UTC");
    }

    let config_file = ConfigFile::load(args.config.as_deref()).map_err(|e| eyre!(e))?;
    let database_url = args
        .db
        .or(config_file.database.url)
        .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_string());
    let data_path = args
        .data
        .or(config_file.server.data_path)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_PATH));
    // Parse the demo data and fetch the songs first, so a broken file or a
    // cancelled download doesn't cost the database.
    let mut demo = DemoData::load(&args.demo).map_err(|e| eyre!(e))?;
    let songs = args
        .songs
        .unwrap_or_else(|| demo.base_dir.join("demo-songs"));
    demo.fetch_songs(&songs).map_err(|e| eyre!(e))?;

    let options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);
    let filename = options.get_filename().to_path_buf();
    if filename.as_os_str() == ":memory:" {
        info!("Using an in-memory database");
    } else {
        warn!("Deleting database {}", filename.display());
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut path = filename.as_os_str().to_owned();
            path.push(suffix);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(eyre!(
                        "Couldn't delete {}: {e}",
                        PathBuf::from(path).display()
                    ));
                }
            }
        }
    }

    let db = sqlx::SqlitePool::connect_with(options).await?;
    info!("Running database migrations");
    if filename.as_os_str() == ":memory:" {
        migrate!("./migrations").run(&db).await?;
    } else {
        einrad_server::db_backup::migrate_with_backup(&db, &filename).await?;
    }

    info!("Applying demo data from {}", args.demo.display());
    demo.apply(&db, &data_path).await.map_err(|e| eyre!(e))?;
    db.close().await;

    info!(
        "Done. Restart a running server (or call /api/command/reload_db) to pick up the new database."
    );
    Ok(())
}
