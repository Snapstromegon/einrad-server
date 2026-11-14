use std::str::FromStr;

use clap::Parser;
use einrad_server::{
    admin_user::insert_admin_user,
    config::{
        Bootstrap, CliArgs, ConfigFile, DEFAULT_CONFIG_PATH, Settings, competition_file_path,
    },
    reloadable_sqlite, seed,
    setup::{self, SetupEnv, SetupState},
    startup,
};
use eyre::eyre;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::FmtSubscriber;

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
    let cli = CliArgs::parse();
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Starting registration system.");
    if !local_offset_known {
        warn!("Couldn't determine the local time zone: backups are named in UTC");
    }
    let config_file = ConfigFile::load(cli.config.as_deref()).map_err(|e| eyre!(e))?;
    let bootstrap = Bootstrap::resolve(&cli, &config_file);
    // An incomplete config doesn't stop the server: it starts in first-run
    // setup (ADR 0011), which needs the database for its admin check.
    // The admin and the competition file count even when the system config
    // is incomplete, so the setup sees the admin as done and a broken file
    // fails now, not in the middle of the wizard.
    let admin = cli.admin.clone().or_else(|| config_file.admin.clone());
    let competition_file = competition_file_path(&cli);
    let setup_cli = cli.clone();
    let config_dir = setup::config_dir(Some(
        cli.config
            .as_deref()
            .unwrap_or(std::path::Path::new(DEFAULT_CONFIG_PATH)),
    ));
    let settings = Settings::resolve(cli, config_file);

    info!("Connecting to database");

    let db = reloadable_sqlite::connect(&bootstrap.database_url)
        .await
        .expect("Couldn't connect to database");

    info!("Running database migrations");
    match sqlx::sqlite::SqliteConnectOptions::from_str(&bootstrap.database_url)?.get_filename() {
        path if path.as_os_str() == ":memory:" => db_sqlite::MIGRATOR.run(&db).await?,
        path => einrad_server::db_backup::migrate_with_backup(&db, path).await?,
    }

    let mut report = None;
    if let Ok(settings) = &settings {
        for warning in &settings.warnings {
            warn!("{warning}");
        }
        if let Some(path) = &settings.competition_file {
            info!("Reading the competition file {}", path.display());
        }
        report = Some(
            seed::apply_settings(&db, settings)
                .await
                .map_err(|e| eyre!(e))?,
        );
    } else if let Some(path) = &competition_file {
        info!("Checking the competition file {}", path.display());
        seed::check_file(path).await.map_err(|e| eyre!(e))?;
    }
    if let Some(admin) = &admin {
        info!("Inserting admin user with email: {}", admin.email);
        insert_admin_user(&db, &admin.name, &admin.email, &admin.password).await?;
    }

    if let Some(reason) = setup::pending_reason(&settings, &db).await? {
        let state = SetupState::new(
            bootstrap.configured_base_url,
            SetupEnv::new(db.clone(), setup_cli),
        );
        state.announce(&reason);
        let listener = tokio::net::TcpListener::bind(&bootstrap.http_address).await?;
        info!("First-run setup on {}", bootstrap.http_address);
        setup::serve(listener, setup::router(state), shutdown_signal()).await?;
        return Ok(());
    }
    let settings = settings.map_err(|e| eyre!(e))?;
    let report = report.expect("the competition file was applied for a complete config");

    let mut normal = startup::build(db, settings, &report, &config_dir).await?;

    info!("Starting HTTP server");
    normal.server.start(shutdown_signal()).await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    info!("Waiting for shutdown signal (Ctrl+C or SIGTERM)");

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
