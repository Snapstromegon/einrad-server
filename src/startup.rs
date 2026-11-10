//! Builds the normal-mode server from resolved settings. The binary does it
//! at a normal start, and the first-run setup does it for the handover into
//! normal mode (ADR 0011), so both run the same code.

use std::sync::Arc;

use eyre::eyre;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::{
    config::Settings,
    http_server::{HttpServer, HttpServerOptions},
    image_fetch::ImageFetcher,
    jwt::JWTConfig,
    mailer::Mailer,
    normalize,
    photos::Photos,
    player::PlayerHub,
    reloadable_sqlite::ReloadableSqlite,
    seed::{self, Report},
};

/// The normal-mode server, ready to serve.
pub struct NormalMode {
    pub server: HttpServer,
    /// The keys the server's sessions are signed with.
    pub jwt: Arc<JWTConfig>,
    /// The address from the settings. A handover keeps the listener it has.
    pub http_address: String,
}

/// Everything a normal start does after the competition file was applied:
/// mailer, the competition (or setup mode when there is none), JWT keys,
/// player hub and background jobs. Nothing is bound or served yet.
///
/// # Errors
/// If the mailer's SMTP server setting is invalid or a component can't start.
pub async fn build(
    db: SqlitePool,
    settings: Settings,
    report: &Report,
) -> eyre::Result<NormalMode> {
    info!("Build Mail client");
    let mailer = Mailer::try_new(
        &settings.smtp_server,
        &settings.smtp_username,
        &settings.smtp_password,
        &settings.smtp_username,
    )
    .map_err(|e| eyre!("The SMTP server setting is invalid: {e}"))?;

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
    let jwt = Arc::new(JWTConfig::new(
        jsonwebtoken::EncodingKey::from_secret(settings.jwt_secret.as_bytes()),
        jsonwebtoken::DecodingKey::from_secret(settings.jwt_secret.as_bytes()),
        jwt_algorithm,
        validator,
        settings.insecure_cookies,
    ));

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

    let http_address = settings.http_address.clone();
    let server = HttpServer::new(
        HttpServerOptions {
            bind_address: settings.http_address,
            base_url: settings.base_url.to_string(),
            data_path: settings.data_path,
            reload_db_token: settings.reload_db_token,
            metrics_token: settings.metrics_token,
            features: settings.features,
            competition_file_id: report.file_id.clone(),
        },
        db,
        jwt.clone(),
        Arc::new(mailer),
        player_hub,
        photos,
    );
    Ok(NormalMode {
        server,
        jwt,
        http_address,
    })
}

#[cfg(feature = "local-audio")]
fn start_local_player(hub: &Arc<PlayerHub>, data_path: &std::path::Path, name: String) {
    crate::player::local::start(hub.clone(), data_path.to_owned(), name);
}

#[cfg(not(feature = "local-audio"))]
fn start_local_player(_hub: &Arc<PlayerHub>, _data_path: &std::path::Path, name: String) {
    warn!(
        "LOCAL_PLAYER is set to {name:?}, but this server was built without the local-audio feature; the server player is off."
    );
}
