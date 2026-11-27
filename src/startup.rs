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
    setup,
};

/// The normal-mode server, ready to serve.
pub struct NormalMode {
    pub server: HttpServer,
    /// The keys the server's sessions are signed with.
    pub jwt: Arc<JWTConfig>,
}

/// Everything a normal start does after the competition file was applied:
/// mailer, the competition (or setup mode when there is none), JWT keys,
/// player hub and background jobs. Nothing is bound or served yet. A setup
/// token file left in `config_dir` is removed: the setup isn't needed.
///
/// # Errors
/// If the mailer's SMTP server setting is invalid or a component can't start.
pub async fn build(
    db: SqlitePool,
    settings: Settings,
    report: &Report,
    config_dir: &std::path::Path,
) -> eyre::Result<NormalMode> {
    setup::remove_stale_token_file(config_dir);
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

    ensure_data_folders(&settings.data_path)?;

    let photos = Photos::new().map_err(|e| eyre!(e))?;
    let player_hub = PlayerHub::new(db.clone());
    if let Some(name) = settings.local_player {
        start_local_player(&player_hub, &settings.data_path, name);
    }
    // URL images (logo, break image, sponsors) are downloaded in the
    // background, so an unreachable server never delays the start (#291).
    ImageFetcher::shared().refresh_in_background(db.clone(), None);
    photos.start(db.clone());

    let server = HttpServer::new(
        HttpServerOptions {
            bind_address: settings.http_address,
            base_url: settings.base_url.to_string(),
            data_path: settings.data_path,
            reload_db_token: settings.reload_db_token,
            metrics_token: settings.metrics_token,
            competition_file_id: report.file_id.clone(),
        },
        db,
        jwt.clone(),
        Arc::new(mailer),
        player_hub,
        photos,
    );
    Ok(NormalMode { server, jwt })
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

/// Folders for Kürfotos under `data_path` (ADR 0026). Created if missing, so
/// a fresh or older data folder needs no manual step.
fn ensure_data_folders(data_path: &std::path::Path) -> eyre::Result<()> {
    for folder in ["photos/original", "photos/resized"] {
        let path = data_path.join(folder);
        std::fs::create_dir_all(&path)
            .map_err(|e| eyre!("cannot create data folder {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_photo_folders_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("data-folders-{}", uuid::Uuid::new_v4()));
        ensure_data_folders(&dir).unwrap();
        ensure_data_folders(&dir).unwrap();
        assert!(dir.join("photos/original").is_dir());
        assert!(dir.join("photos/resized").is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
