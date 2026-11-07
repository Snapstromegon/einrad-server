//! Startup with a competition file: the CLI flags, the files and the
//! database as the server binary uses them (ADR 0005).

mod common;

use clap::Parser;
use common::{TempDir, in_memory_migrated};
use einrad_server::{
    competition::Competition,
    config::{CliArgs, ConfigFile, Settings},
    logo,
    seed::{self, Applied, Report},
};
use sqlx::SqlitePool;

const SYSTEM_CONFIG: &str = r#"
[smtp]
server = "smtp.example.org"
user = "cup@example.org"
password = "secret"
"#;

const COMPETITION: &str = include_str!("fixtures/competition.toml");

/// Starts the server's startup path with `args` (after the binary name)
/// against `db`, as far as the competition goes.
async fn start(db: &SqlitePool, dir: &TempDir, args: &[&str]) -> Result<Report, String> {
    let config = dir.0.join("config.toml");
    let data = dir.0.join("data");
    let mut argv = vec![
        "registration_server".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--data".to_string(),
        data.display().to_string(),
    ];
    argv.extend(args.iter().map(|arg| arg.to_string()));
    let cli = CliArgs::try_parse_from(argv).map_err(|e| e.to_string())?;
    let file = ConfigFile::load(cli.config.as_deref())?;
    let settings = Settings::resolve(cli, file)?;
    seed::apply_settings(db, &settings).await
}

fn write(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.0.join(name);
    std::fs::write(&path, content).unwrap();
    path.display().to_string()
}

#[tokio::test]
async fn a_second_start_with_a_changed_file_keeps_the_competition_and_warns() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let competition = write(&dir, "competition.toml", COMPETITION);

    let first = start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert_eq!(first.applied, Applied::Created);
    let created = Competition::current(&db).await.unwrap().unwrap();

    write(
        &dir,
        "competition.toml",
        &COMPETITION.replacen("fee_per_start = 15", "fee_per_start = 20", 1),
    );
    let second = start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert_eq!(second.applied, Applied::Ignored);
    assert!(
        second
            .warnings
            .iter()
            .any(|w| w.contains("--replace-competition")),
        "{:?}",
        second.warnings
    );
    assert_eq!(Competition::current(&db).await.unwrap().unwrap(), created);
}

#[tokio::test]
async fn replace_competition_applies_the_changed_file() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let competition = write(&dir, "competition.toml", COMPETITION);
    start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();

    write(
        &dir,
        "competition.toml",
        &COMPETITION.replacen("fee_per_start = 15", "fee_per_start = 20", 1),
    );
    let report = start(
        &db,
        &dir,
        &["--competition", &competition, "--replace-competition"],
    )
    .await
    .unwrap();
    assert!(
        matches!(report.applied, Applied::Replaced(_)),
        "{:?}",
        report.applied
    );
    let fee = Competition::current(&db)
        .await
        .unwrap()
        .unwrap()
        .fee_per_start;
    assert_eq!(fee, 20.0);
}

#[tokio::test]
async fn features_and_photos_come_with_the_competition() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let settings = "\n[competition.features]\njury_registration = false\n\
        [competition.photos]\nsource = \"https://cloud.example.org/s/AbC123\"\npassword = \"geheim\"\n";
    let competition = write(
        &dir,
        "competition.toml",
        &format!("{COMPETITION}{settings}"),
    );
    start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    let created = Competition::current(&db).await.unwrap().unwrap();
    assert_eq!(created.features.jury_registration, Some(false));
    let photos = created.photos.unwrap();
    assert_eq!(photos.source, "https://cloud.example.org/s/AbC123");
    assert_eq!(photos.password.as_deref(), Some("geheim"));

    // A changed photo password counts as a difference.
    write(
        &dir,
        "competition.toml",
        &format!("{COMPETITION}{}", settings.replace("geheim", "neu")),
    );
    let second = start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert_eq!(second.applied, Applied::Ignored);
    assert!(
        second
            .warnings
            .iter()
            .any(|w| w.contains("the competition settings")),
        "{:?}",
        second.warnings
    );
}

/// The test competition file with `logo_svg` set to `value` (TOML).
fn with_logo(value: &str) -> String {
    COMPETITION.replacen(
        "max_fee_per_rider = 45\n",
        &format!("max_fee_per_rider = 45\nlogo_svg = {value}\n"),
        1,
    )
}

const SVG_LOGO: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"/>"#;

#[tokio::test]
async fn a_logo_comes_with_the_file_and_stays_unless_a_replace_sets_it() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let competition = write(
        &dir,
        "competition.toml",
        &with_logo(&format!("'{SVG_LOGO}'")),
    );
    start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    let logo = |db| async move { logo::load(&db).await.unwrap().map(|logo| logo.data) };
    assert_eq!(logo(db.clone()).await, Some(SVG_LOGO.as_bytes().to_vec()));

    // A changed logo is a difference like any other.
    let other = SVG_LOGO.replace("10 10", "20 20");
    write(&dir, "competition.toml", &with_logo(&format!("'{other}'")));
    let second = start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert_eq!(second.applied, Applied::Ignored);
    assert!(
        second.warnings.iter().any(|w| w.contains("the logo")),
        "{:?}",
        second.warnings
    );

    // A file without one keeps it, even on a replace.
    write(&dir, "competition.toml", COMPETITION);
    let replace = [
        "--competition",
        competition.as_str(),
        "--replace-competition",
    ];
    let report = start(&db, &dir, &replace).await.unwrap();
    assert_eq!(report.applied, Applied::Unchanged);
    assert_eq!(logo(db.clone()).await, Some(SVG_LOGO.as_bytes().to_vec()));

    // An empty logo_svg resets it to the built-in default.
    write(&dir, "competition.toml", &with_logo("\"\""));
    let report = start(&db, &dir, &replace).await.unwrap();
    assert!(
        matches!(report.applied, Applied::Replaced(_)),
        "{:?}",
        report.applied
    );
    assert_eq!(logo(db.clone()).await, None);
}

#[tokio::test]
async fn a_file_with_other_colors_is_ignored_with_a_warning_naming_them() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let with_color = |color: &str| {
        COMPETITION.replacen(
            "max_fee_per_rider = 45\n",
            &format!("max_fee_per_rider = 45\nprimary_color = \"{color}\"\n"),
            1,
        )
    };
    let competition = write(&dir, "competition.toml", &with_color("#cc0055"));
    start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    let primary = |db| async move {
        Competition::current(&db)
            .await
            .unwrap()
            .and_then(|c| c.primary_color)
            .map(|color| color.to_string())
    };
    assert_eq!(primary(db.clone()).await.as_deref(), Some("#cc0055"));

    write(&dir, "competition.toml", &with_color("#1a6b3c"));
    let report = start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert_eq!(report.applied, Applied::Ignored);
    assert!(
        report.warnings.iter().any(|w| w.contains("the colors")),
        "{:?}",
        report.warnings
    );
    assert_eq!(primary(db.clone()).await.as_deref(), Some("#cc0055"));
}

#[tokio::test]
async fn photos_have_no_flags_or_environment_variables_anymore() {
    let err = CliArgs::try_parse_from(["registration_server", "--photos-source", "/srv/fotos"])
        .unwrap_err();
    assert!(err.to_string().contains("--photos-source"), "{err}");
}

#[tokio::test]
async fn without_a_competition_an_admin_lets_the_server_start_in_setup_mode() {
    let db = in_memory_migrated().await;
    let err = seed::startup_competition(&db).await.unwrap_err();
    assert!(err.contains("setup mode"), "{err}");

    sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES (?, 'admin@cup.test', true, 'Admin', 'x')")
        .bind(uuid::Uuid::now_v7())
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_roles (user_id, role) SELECT id, 'admin' FROM users")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(seed::startup_competition(&db).await.unwrap(), None);

    let dir = TempDir::new();
    write(&dir, "config.toml", SYSTEM_CONFIG);
    let competition = write(&dir, "competition.toml", COMPETITION);
    start(&db, &dir, &["--competition", &competition])
        .await
        .unwrap();
    assert!(seed::startup_competition(&db).await.unwrap().is_some());
}

#[tokio::test]
async fn a_combined_config_stops_the_start() {
    let db = in_memory_migrated().await;
    let dir = TempDir::new();
    write(
        &dir,
        "config.toml",
        &format!("{SYSTEM_CONFIG}\n{COMPETITION}"),
    );

    let err = start(&db, &dir, &[]).await.unwrap_err();
    assert!(err.contains("competition file"), "{err}");
    assert_eq!(Competition::current(&db).await.unwrap(), None);
}
