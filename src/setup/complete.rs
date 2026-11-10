//! The setup API's logic (ADR 0011): which steps are missing, the test mail
//! and the completion that writes `config.toml` and creates the admin.
//!
//! Nothing here returns or logs a secret except the one-time metrics token
//! of a completion and the content of a config that couldn't be written (for
//! the user to save by hand).

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{info, warn};
use url::Url;

use super::SetupState;
use crate::{
    admin_user,
    config::{CliArgs, ConfigFile, DEFAULT_CONFIG_PATH, Settings, non_blank},
    mailer::Mailer,
    utils::check_password,
};

const TEST_MAIL_TIMEOUT: Duration = Duration::from_secs(20);

/// What a pending setup works on: the database, the CLI/environment values
/// (which are never written) and where the config file goes.
pub struct SetupEnv {
    pub db: SqlitePool,
    pub cli: CliArgs,
    /// `--config`/`CONFIG`, default `./config.toml`.
    pub config_path: PathBuf,
}

impl SetupEnv {
    #[must_use]
    pub fn new(db: SqlitePool, cli: CliArgs) -> Self {
        let config_path = cli
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
        Self {
            db,
            cli,
            config_path,
        }
    }

    /// The config file as it is on disk now; a missing file is an empty
    /// config. Read fresh each time, so a file the user saved by hand counts.
    fn load_file(&self) -> Result<ConfigFile, String> {
        // No parser message: it would quote the offending line, which may
        // be a secret.
        ConfigFile::load(Some(&self.config_path)).map_err(|_| {
            format!(
                "The config file {} can't be read or parsed. Fix it and reload.",
                self.config_path.display()
            )
        })
    }
}

/// Where a value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Not set: the setup can fill it in.
    Missing,
    /// Set by a CLI flag or the environment: read-only, never written.
    Env,
    /// Set in the config file, or (for the admin) in the database.
    Done,
}

impl Source {
    fn of(cli: bool, file: bool) -> Self {
        if cli {
            Self::Env
        } else if file {
            Self::Done
        } else {
            Self::Missing
        }
    }

    fn missing(self) -> bool {
        self == Self::Missing
    }

    /// The state of a step made of several values: missing if any is, `Env`
    /// if all come from the environment.
    fn combine(parts: &[Self]) -> Self {
        if parts.iter().any(|p| p.missing()) {
            Self::Missing
        } else if parts.iter().all(|p| *p == Self::Env) {
            Self::Env
        } else {
            Self::Done
        }
    }
}

/// What is set and what isn't, from CLI/environment, config file and database.
struct Snapshot {
    file: ConfigFile,
    file_exists: bool,
    /// Whether the system config is incomplete, so the setup writes a file.
    needs_file: bool,
    base_url: Source,
    smtp_server: Source,
    smtp_user: Source,
    smtp_password: Source,
    jwt_secret: Source,
    reload_db_token: Source,
    metrics_token: Source,
    admin: Source,
}

impl Snapshot {
    async fn take(env: &SetupEnv) -> Result<Self, String> {
        let file = env.load_file()?;
        let cli = &env.cli;
        let nb = |v: &Option<String>| non_blank(v.clone()).is_some();
        let admin = admin_user::admin_exists(&env.db)
            .await
            .map_err(|e| format!("Couldn't read the database: {e}"))?;
        Ok(Self {
            needs_file: Settings::resolve(cli.clone(), file.clone()).is_err(),
            file_exists: env.config_path.exists(),
            base_url: Source::of(cli.base_url.is_some(), file.server.base_url.is_some()),
            smtp_server: Source::of(nb(&cli.smtp_server), nb(&file.smtp.server)),
            smtp_user: Source::of(nb(&cli.smtp_username), nb(&file.smtp.user)),
            smtp_password: Source::of(nb(&cli.smtp_password), nb(&file.smtp.password)),
            jwt_secret: Source::of(nb(&cli.jwt_secret), nb(&file.auth.jwt_secret)),
            reload_db_token: Source::of(nb(&cli.reload_db_token), nb(&file.server.reload_db_token)),
            metrics_token: Source::of(
                cli.metrics_token.as_deref().is_some_and(|t| !t.is_empty()),
                file.server
                    .metrics_token
                    .as_deref()
                    .is_some_and(|t| !t.is_empty()),
            ),
            admin: if admin { Source::Done } else { Source::Missing },
            file,
        })
    }
}

#[derive(Serialize)]
pub struct StepStatus {
    pub state: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct Steps {
    pub base_url: StepStatus,
    pub smtp: StepStatus,
    pub secrets: StepStatus,
    pub admin: StepStatus,
}

/// The status of the setup. It holds states only, never values.
#[derive(Serialize)]
pub struct Status {
    /// Whether the completion writes a config file. If not, the system
    /// config is complete and only the admin is created.
    pub writes_config: bool,
    /// `will_write`, `not_needed`, or `exists_incomplete`: a file is already
    /// at `config_path` and lacks settings, so the setup can't write one.
    pub config: &'static str,
    pub config_path: PathBuf,
    pub steps: Steps,
}

fn step(state: Source, fields: Option<serde_json::Value>) -> StepStatus {
    StepStatus { state, fields }
}

/// # Errors
/// If the config file is broken or the database can't be read.
pub async fn status(env: &SetupEnv) -> Result<Status, String> {
    let s = Snapshot::take(env).await?;
    let smtp = [s.smtp_server, s.smtp_user, s.smtp_password];
    let secrets = [s.jwt_secret, s.reload_db_token];
    Ok(Status {
        writes_config: s.needs_file,
        config: match (s.needs_file, s.file_exists) {
            (false, _) => "not_needed",
            (true, true) => "exists_incomplete",
            (true, false) => "will_write",
        },
        config_path: absolute(&env.config_path),
        steps: Steps {
            base_url: step(s.base_url, None),
            smtp: step(
                Source::combine(&smtp),
                Some(serde_json::json!({
                    "server": s.smtp_server, "user": s.smtp_user, "password": s.smtp_password,
                })),
            ),
            secrets: step(
                Source::combine(&secrets),
                Some(serde_json::json!({
                    "jwt_secret": s.jwt_secret,
                    "reload_db_token": s.reload_db_token,
                    // Opt-in: doesn't count towards the step's state.
                    "metrics_token": s.metrics_token,
                })),
            ),
            admin: step(s.admin, None),
        },
    })
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SmtpInput {
    pub server: String,
    pub user: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct TestMailInput {
    #[serde(default)]
    pub smtp: SmtpInput,
    pub to: String,
}

/// Sends a test mail with the effective SMTP settings: what the environment
/// or the file already sets, and the input for the rest.
///
/// # Errors
/// The reason the mail wasn't sent, as text for the user.
pub async fn test_mail(env: &SetupEnv, input: TestMailInput) -> Result<(), String> {
    let file = env.load_file()?;
    let cli = &env.cli;
    // Stored credentials only go to a stored server; with a server from the
    // input, the credentials come from the input too.
    let stored_server =
        non_blank(cli.smtp_server.clone()).or_else(|| non_blank(file.smtp.server.clone()));
    let pick = |cli: &Option<String>, file: &Option<String>, input: &str| {
        let stored = if stored_server.is_some() {
            non_blank(cli.clone()).or_else(|| non_blank(file.clone()))
        } else {
            None
        };
        stored.or_else(|| non_blank(Some(input.to_string())))
    };
    let server = stored_server
        .clone()
        .or_else(|| non_blank(Some(input.smtp.server.clone())));
    let user = pick(&cli.smtp_username, &file.smtp.user, &input.smtp.user);
    let password = pick(&cli.smtp_password, &file.smtp.password, &input.smtp.password);
    let (Some(server), Some(user), Some(password)) = (server, user, password) else {
        return Err("SMTP server, user and password are needed for a test mail.".into());
    };
    let mailer = build_mailer(&server, &user, &password)?;
    let sent = tokio::time::timeout(
        TEST_MAIL_TIMEOUT,
        mailer.send_text(
            input.to.trim(),
            "Testmail der Ersteinrichtung",
            "Diese Mail bestätigt, dass der Mailversand des Servers funktioniert.",
        ),
    )
    .await;
    match sent {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("The mail server didn't answer in time.".into()),
    }
}

/// The mailer the server builds from these settings (the sender is the SMTP
/// user), checked so that it can't panic.
fn build_mailer(server: &str, user: &str, password: &str) -> Result<Mailer, String> {
    user.parse::<lettre::message::Mailbox>().map_err(|_| {
        format!("The SMTP user {user:?} isn't an email address, which mails need as sender.")
    })?;
    Mailer::try_new(server, user, password, user)
        .map_err(|e| format!("The SMTP server setting is invalid: {e}"))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AdminInput {
    pub name: String,
    pub email: String,
    pub password: String,
    pub password_repeat: String,
}

/// What the wizard submits. Only what a step still misses is used.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CompleteInput {
    pub base_url: String,
    pub smtp: SmtpInput,
    /// Opt in to a generated metrics token.
    pub metrics: bool,
    pub admin: AdminInput,
}

/// What a completion did.
#[derive(Serialize)]
pub struct Completed {
    /// Shown once, if the user opted in and it was generated.
    pub metrics_token: Option<String>,
    pub config_written: bool,
    pub config_path: PathBuf,
    /// Until the in-process handover exists, the server has to be restarted.
    pub restart_required: bool,
}

pub enum CompleteError {
    AlreadyDone,
    /// The input is invalid; nothing was written.
    Invalid(Vec<String>),
    /// The config file can't be written; its content is for the user to save
    /// at `path`. Nothing was written and no admin was created.
    Conflict {
        reason: &'static str,
        path: PathBuf,
        content: String,
        /// A file is already there: `content` is only the missing settings,
        /// to add to it, not a replacement.
        partial: bool,
    },
    Failed(String),
}

/// The values the setup writes into a new config file.
#[derive(Default)]
struct ToWrite {
    base_url: Option<String>,
    smtp_server: Option<String>,
    smtp_user: Option<String>,
    smtp_password: Option<String>,
    jwt_secret: Option<String>,
    reload_db_token: Option<String>,
    metrics_token: Option<String>,
}

/// 32 random bytes as base64url without padding.
fn random_token() -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system has no source of randomness");
    let mut out = String::with_capacity(43);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

/// The config file: the values to write, and the rest of
/// `config.example.toml` commented out.
fn render(w: &ToWrite) -> String {
    let mut out = String::from(
        "# System config, written by the first-run setup.\n\
         #\n\
         # Every setting can also be set by a CLI flag or environment variable, which take\n\
         # precedence over this file; see config.example.toml.\n\
         #\n\
         # The admin was created by the first-run setup and lives in the database, not in\n\
         # this file.\n",
    );
    let line = |out: &mut String, key: &str, value: &Option<String>| {
        if let Some(value) = value {
            out.push_str(&format!("{key} = {}\n", toml_string(value)));
        }
    };
    out.push_str("\n[server]\n");
    line(&mut out, "base_url", &w.base_url);
    line(&mut out, "reload_db_token", &w.reload_db_token);
    if w.metrics_token.is_some() {
        line(&mut out, "metrics_token", &w.metrics_token);
    } else {
        out.push_str("# metrics_token = \"…\"                              # METRICS_TOKEN\n");
    }
    out.push_str(
        "# http_address = \"127.0.0.1:3000\"                    # HTTP_ADDRESS\n\
         # data_path = \"./data\"                               # DATA_PATH\n\
         # insecure_cookies = false                           # INSECURE_COOKIES\n\
         # local_player = \"Halle\"                             # LOCAL_PLAYER\n",
    );
    out.push_str(
        "\n[database]\n# url = \"sqlite://./db.sqlite?mode=rwc\"              # DATABASE_URL\n",
    );
    out.push_str("\n[smtp]\n");
    line(&mut out, "server", &w.smtp_server);
    line(&mut out, "user", &w.smtp_user);
    line(&mut out, "password", &w.smtp_password);
    out.push_str("\n[auth]\n");
    line(&mut out, "jwt_secret", &w.jwt_secret);
    out.push_str(
        "\n# Features that are still being refined; a competition turns each on or off.\n\
         # [features]\n\
         # jury_registration = true                           # JURY_REGISTRATION\n\
         # moderation_input = true                            # MODERATION_INPUT\n",
    );
    out
}

/// Fills what `base` leaves unset with the values of `add`.
fn overlay(mut base: ConfigFile, add: ConfigFile) -> ConfigFile {
    base.server.base_url = base.server.base_url.or(add.server.base_url);
    base.server.reload_db_token = base.server.reload_db_token.or(add.server.reload_db_token);
    base.server.metrics_token = base.server.metrics_token.or(add.server.metrics_token);
    base.smtp.server = base.smtp.server.or(add.smtp.server);
    base.smtp.user = base.smtp.user.or(add.smtp.user);
    base.smtp.password = base.smtp.password.or(add.smtp.password);
    base.auth.jwt_secret = base.auth.jwt_secret.or(add.auth.jwt_secret);
    base
}

enum WriteError {
    Exists,
    Other(std::io::Error),
}

/// Writes `content` to `path` (mode 0600) atomically, never replacing an
/// existing file: a temp file next to it is hard-linked to `path`, which
/// fails if `path` exists. (A rename would replace it.) A filesystem without
/// hard links fails as `Other`, and the setup offers the content instead.
fn write_new_file(path: &Path, content: &str) -> Result<(), WriteError> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(WriteError::Exists);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", &random_token()[..12]));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::hard_link(&temp, path)
    })();
    let _ = std::fs::remove_file(&temp);
    result.map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => WriteError::Exists,
        _ => WriteError::Other(e),
    })
}

/// Completes the setup behind one lock; see ADR 0011 for the order.
pub async fn complete(
    state: &SetupState,
    input: CompleteInput,
) -> Result<Completed, CompleteError> {
    let mut done = state.completion.lock().await;
    if *done {
        return Err(CompleteError::AlreadyDone);
    }
    let env = &state.env;
    let snap = Snapshot::take(env).await.map_err(CompleteError::Failed)?;
    let mut errors = Vec::new();

    // 1. Validate everything in memory.
    let mut write = ToWrite::default();
    if snap.needs_file {
        let text = |value: &str| non_blank(Some(value.trim().to_string()));
        if snap.base_url.missing() {
            match input.base_url.trim().parse::<Url>() {
                Ok(url) if matches!(url.scheme(), "http" | "https") && url.has_host() => {
                    write.base_url = Some(url.to_string());
                }
                _ => errors.push("base_url: needs an http(s) URL".to_string()),
            }
        }
        for (missing, value, slot, name) in [
            (
                snap.smtp_server,
                &input.smtp.server,
                &mut write.smtp_server,
                "smtp.server",
            ),
            (
                snap.smtp_user,
                &input.smtp.user,
                &mut write.smtp_user,
                "smtp.user",
            ),
            (
                snap.smtp_password,
                &input.smtp.password,
                &mut write.smtp_password,
                "smtp.password",
            ),
        ] {
            if missing.missing() {
                // The password keeps its spaces.
                let v = if name == "smtp.password" {
                    non_blank(Some(value.clone()))
                } else {
                    text(value)
                };
                match v {
                    Some(v) => *slot = Some(v),
                    None => errors.push(format!("{name}: is required")),
                }
            }
        }
        if snap.jwt_secret.missing() {
            write.jwt_secret = Some(random_token());
        }
        if snap.reload_db_token.missing() {
            write.reload_db_token = Some(random_token());
        }
        if input.metrics && snap.metrics_token.missing() {
            write.metrics_token = Some(random_token());
        }
    }
    let mut admin = None;
    if snap.admin.missing() {
        let a = &input.admin;
        let name = a.name.trim();
        let email = a.email.trim();
        if name.is_empty() {
            errors.push("admin.name: is required".to_string());
        }
        if email.parse::<lettre::Address>().is_err() {
            errors.push("admin.email: needs an email address".to_string());
        } else if admin_user::email_in_use(&env.db, email)
            .await
            .map_err(|e| CompleteError::Failed(format!("Couldn't read the database: {e}")))?
        {
            errors.push("admin.email: is already used by another user".to_string());
        }
        if a.password != a.password_repeat {
            errors.push("admin.password_repeat: the passwords differ".to_string());
        } else if let Err(e) = check_password(&a.password) {
            errors.push(format!("admin.password: {e}"));
        }
        admin = Some((name.to_string(), email.to_string(), a.password.clone()));
    }
    let mut content = None;
    if snap.needs_file && errors.is_empty() {
        let rendered = render(&write);
        // What the file will say must resolve to complete settings.
        let resolved = ConfigFile::parse(&rendered)
            .map_err(|e| format!("the written config doesn't parse: {e}"))
            .and_then(|parsed| {
                Settings::resolve(env.cli.clone(), overlay(snap.file.clone(), parsed))
            });
        match resolved {
            Ok(settings) => {
                if let Err(e) = build_mailer(
                    &settings.smtp_server,
                    &settings.smtp_username,
                    &settings.smtp_password,
                ) {
                    errors.push(format!("smtp: {e}"));
                }
            }
            Err(e) => errors.push(e),
        }
        content = Some(rendered);
    }
    if !errors.is_empty() {
        return Err(CompleteError::Invalid(errors));
    }

    if snap.needs_file && snap.file_exists {
        // Known now; no need to hash a password first.
        return Err(CompleteError::Conflict {
            reason: "exists",
            partial: true,
            path: absolute(&env.config_path),
            content: content.unwrap_or_default(),
        });
    }

    // Hashing is slow; do it before anything is written.
    let hashed = match admin {
        Some((name, email, password)) => {
            let hash = tokio::task::spawn_blocking(move || password_auth::generate_hash(password))
                .await
                .map_err(|e| CompleteError::Failed(e.to_string()))?;
            Some((name, email, hash))
        }
        None => None,
    };

    // 2. The config file, if one is needed.
    let path = absolute(&env.config_path);
    if let Some(content) = content {
        let reason = match write_new_file(&path, &content) {
            Ok(()) => None,
            Err(WriteError::Exists) => Some("exists"),
            Err(WriteError::Other(e)) => {
                warn!("Couldn't write the config file {}: {e}", path.display());
                Some("not_writable")
            }
        };
        if let Some(reason) = reason {
            return Err(CompleteError::Conflict {
                reason,
                partial: snap.file_exists,
                path,
                content,
            });
        }
        info!("Wrote the config file {}", path.display());
    }

    // 3. The admin. A failure leaves what the trigger rule already covers:
    // the next start (or status) shows only the admin as missing.
    if let Some((name, email, hash)) = hashed {
        admin_user::insert_admin_with_hash(&env.db, &name, &email, &hash)
            .await
            .map_err(|e| CompleteError::Failed(format!("Couldn't create the admin: {e}")))?;
        info!("Created the admin {email}");
    }

    // 4. The token file goes away; the setup is done.
    state.finish();
    *done = true;
    Ok(Completed {
        metrics_token: write.metrics_token,
        config_written: snap.needs_file,
        config_path: path,
        restart_required: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_tokens_are_43_url_safe_characters() {
        let a = random_token();
        assert_eq!(a.len(), 43);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(a, random_token());
    }
}
