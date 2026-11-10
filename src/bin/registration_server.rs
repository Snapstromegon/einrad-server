use clap::Parser;
use einrad_server::{
    config::{Bootstrap, CliArgs, ConfigFile, Settings},
    http_server::{HttpServer, HttpServerOptions},
    image_fetch::ImageFetcher,
    jwt::JWTConfig,
    mailer::Mailer,
    normalize,
    photos::Photos,
    player::PlayerHub,
    reloadable_sqlite::{self, ReloadableSqlite},
    roles::Role,
    seed,
    setup::{self, SetupState},
};
use eyre::eyre;
use password_auth::generate_hash;
use sqlx::migrate;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[tokio::main]
async fn main() -> eyre::Result<()> {
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
    let config_file = ConfigFile::load(cli.config.as_deref()).map_err(|e| eyre!(e))?;
    let bootstrap = Bootstrap::resolve(&cli, &config_file);
    let config_dir = setup::config_dir(cli.config.as_deref());
    // An incomplete config doesn't stop the server: it starts in first-run
    // setup (ADR 0011), which needs the database for its admin check.
    let settings = Settings::resolve(cli, config_file);

    info!("Connecting to database");

    let db = reloadable_sqlite::connect(&bootstrap.database_url)
        .await
        .expect("Couldn't connect to database");

    info!("Running database migrations");
    migrate!("./migrations").run(&db).await?;

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
        if let Some(admin) = &settings.admin {
            info!("Inserting admin user with email: {}", admin.email);
            insert_admin_user(&db, &admin.name, &admin.email, &admin.password).await?;
        }
    }

    if let Some(reason) = setup::pending_reason(&settings, &db).await? {
        let state = SetupState::new(bootstrap.configured_base_url, &config_dir);
        state.announce(&reason);
        let listener = tokio::net::TcpListener::bind(&bootstrap.http_address).await?;
        info!("First-run setup on {}", bootstrap.http_address);
        setup::serve(listener, setup::router(state), shutdown_signal()).await?;
        return Ok(());
    }
    let settings = settings.map_err(|e| eyre!(e))?;
    let report = report.expect("the competition file was applied for a complete config");

    info!("Build Mail client");

    let mailer = Mailer::new(
        &settings.smtp_server,
        &settings.smtp_username,
        &settings.smtp_password,
        &settings.smtp_username,
    );

    match seed::startup_competition(&db).await.map_err(|e| eyre!(e))? {
        Some(competition) => info!(
            "Running competition {} ({})",
            competition.name, competition.id
        ),
        None => warn!(
            "No competition yet: starting in setup mode. An admin imports the competition file \
             in the admin UI (Wettkampf)."
        ),
    }

    info!("Build JWT config");
    let jwt_algorithm = jsonwebtoken::Algorithm::HS512;
    let mut validator = jsonwebtoken::Validation::new(jwt_algorithm);
    validator.validate_aud = false;
    let jwt_config = JWTConfig::new(
        jsonwebtoken::EncodingKey::from_secret(settings.jwt_secret.as_bytes()),
        jsonwebtoken::DecodingKey::from_secret(settings.jwt_secret.as_bytes()),
        jwt_algorithm,
        validator,
        settings.insecure_cookies,
    );

    info!("Starting HTTP server");
    let db = ReloadableSqlite::new(db, settings.database_url);
    // Songs uploaded while the server was down or before normalization existed.
    normalize::resume(&db, &settings.data_path).await?;

    let player_hub = PlayerHub::new(db.clone());
    if let Some(name) = settings.local_player {
        start_local_player(&player_hub, &settings.data_path, name);
    }
    // URL images (logo, break image, sponsors) are downloaded in the
    // background, so an unreachable server never delays the start (#291).
    ImageFetcher::shared().refresh_in_background(db.clone(), None);
    let photos = Photos::new().map_err(|e| eyre!(e))?;
    photos.start(db.clone());

    HttpServer::new(
        HttpServerOptions {
            bind_address: settings.http_address,
            base_url: settings.base_url.to_string(),
            data_path: settings.data_path,
            reload_db_token: settings.reload_db_token,
            metrics_token: settings.metrics_token,
            features: settings.features,
            competition_file_id: report.file_id,
        },
        db,
        Arc::new(jwt_config),
        Arc::new(mailer),
        player_hub,
        photos,
    )
    .start(shutdown_signal())
    .await?;

    Ok(())
}

async fn admin_user_exists(db: &sqlx::SqlitePool) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"
        SELECT COUNT(*) as count FROM user_roles WHERE role = ?
        "#,
        Role::Admin,
    )
    .fetch_one(db)
    .await?;
    Ok(result.count > 0)
}

async fn insert_admin_user(
    db: &sqlx::SqlitePool,
    name: &str,
    email: &str,
    password: &str,
) -> sqlx::Result<()> {
    if admin_user_exists(db).await? {
        return Ok(());
    }
    let hash = generate_hash(password);
    let id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO users (id, email, email_verified, name, password)
        VALUES (?, ?, true, ?, ?)
        "#,
        id,
        email,
        name,
        hash
    )
    .execute(db)
    .await?;
    sqlx::query!(
        "INSERT INTO user_roles (user_id, role) VALUES (?, ?)",
        id,
        Role::Admin,
    )
    .execute(db)
    .await?;
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

#[cfg(feature = "local-audio")]
fn start_local_player(hub: &Arc<PlayerHub>, data_path: &std::path::Path, name: String) {
    einrad_server::player::local::start(hub.clone(), data_path.to_owned(), name);
}

#[cfg(not(feature = "local-audio"))]
fn start_local_player(_hub: &Arc<PlayerHub>, _data_path: &std::path::Path, name: String) {
    warn!(
        "LOCAL_PLAYER is set to {name:?}, but this server was built without the local-audio feature; the server player is off."
    );
}
