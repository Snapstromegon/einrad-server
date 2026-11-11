//! Configuration from CLI flags, environment variables and an optional TOML file.
//!
//! Precedence: CLI flag > environment variable (including `.env`) > config file >
//! built-in default; see `config.example.toml`. The competition (`[competition]`,
//! `[[categories]]`, `[[timeplan]]`, …) comes from a separate competition file,
//! which is only an import into the database (see `crate::seed` and
//! `competition.example.toml`).

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use time::{
    Date, OffsetDateTime, format_description::well_known::Rfc3339, macros::format_description,
};
use url::Url;

use crate::{
    act_type::ActType,
    colors::HexColor,
    competition::Features,
    images::{self, ImageUrl},
    logo::SvgLogo,
    photos::PhotosSettings,
    timeplan::EntryKind,
};

pub const DEFAULT_CONFIG_PATH: &str = "./config.toml";
pub const DEFAULT_COMPETITION_PATH: &str = "./competition.toml";
pub const DEFAULT_BASE_URL: &str = "http://localhost:3000";
pub const DEFAULT_HTTP_ADDRESS: &str = "127.0.0.1:3000";
pub const DEFAULT_DATABASE_URL: &str = "sqlite://./db.sqlite?mode=rwc";
pub const DEFAULT_DATA_PATH: &str = "./data";

/// CLI flags and environment variables. Every value is optional here, so that
/// the config file can fill in what's missing.
#[derive(Debug, Default, Clone, clap::Parser)]
pub struct CliArgs {
    /// Path to the TOML config file. Defaults to `./config.toml` if it exists.
    #[clap(long, env = "CONFIG")]
    pub config: Option<PathBuf>,
    /// Path to the competition file. Defaults to `./competition.toml` if it
    /// exists. Only applied if the database has no competition yet.
    #[clap(long, env = "COMPETITION")]
    pub competition: Option<PathBuf>,
    /// Replaces the database's competition with the competition file, keeping
    /// the club data (see `crate::seed::apply`).
    #[clap(long, env = "REPLACE_COMPETITION")]
    pub replace_competition: bool,
    #[clap(long, env = "BASE_URL")]
    pub base_url: Option<Url>,
    #[clap(long, env = "HTTP_ADDRESS")]
    pub http_address: Option<String>,
    #[clap(long, env = "SMTP_SERVER")]
    pub smtp_server: Option<String>,
    #[clap(long, env = "SMTP_USER")]
    pub smtp_username: Option<String>,
    #[clap(long, env = "SMTP_PASSWORD")]
    pub smtp_password: Option<String>,
    #[clap(long, env = "DATABASE_URL")]
    pub db: Option<String>,
    #[clap(long, env = "DATA_PATH")]
    pub data: Option<PathBuf>,
    #[clap(long, env = "JWT_SECRET")]
    pub jwt_secret: Option<String>,
    /// JSON object with `name`, `email` and `password`.
    #[clap(long, env = "ADMIN")]
    pub admin: Option<AdminConfig>,
    #[clap(long, env = "INSECURE_COOKIES")]
    pub insecure_cookies: bool,
    #[clap(long, env = "RELOAD_DB_TOKEN")]
    pub reload_db_token: Option<String>,
    /// Bearer token for `GET /metrics` (Prometheus); the endpoint is off
    /// without one.
    #[clap(long, env = "METRICS_TOKEN")]
    pub metrics_token: Option<String>,
    /// Name of the server player, which plays music on this server's sound
    /// card (remote playback). Needs a build with the `local-audio` feature.
    #[clap(long, env = "LOCAL_PLAYER")]
    pub local_player: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigFile {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub smtp: SmtpConfig,
    pub auth: AuthConfig,
    pub admin: Option<AdminConfig>,
}

/// The sections of the competition file, which older system configs
/// contained.
const COMPETITION_SECTIONS: [&str; 5] = [
    "competition",
    "categories",
    "finals",
    "timeplan",
    "jury_categories",
];

impl ConfigFile {
    /// Parses a system config. A config that still contains competition
    /// sections is rejected with how to split it.
    pub fn parse(content: &str) -> Result<Self, String> {
        let table: toml::Table = toml::from_str(content).map_err(|e| e.to_string())?;
        let moved = COMPETITION_SECTIONS
            .iter()
            .filter(|section| table.contains_key(**section))
            .map(|section| format!("[{section}]"))
            .collect::<Vec<_>>();
        if !moved.is_empty() {
            return Err(format!(
                "{} belong in the competition file now, not in the system config. \
                 Move them into a file of their own (e.g. competition.toml, see \
                 competition.example.toml) and pass it with --competition or COMPETITION; \
                 [server], [database], [smtp], [auth] and [admin] stay \
                 in the system config.",
                moved.join(", ")
            ));
        }
        if table.contains_key("features") {
            return Err(
                "[features] is a competition setting now: move it into the competition \
                 file as [competition.features], or set it in the admin UI under Wettkampf \
                 bearbeiten. A feature the competition leaves unset follows the build default."
                    .to_string(),
            );
        }
        if table.contains_key("photos") {
            return Err(
                "[photos] is a competition setting now: move it into the competition \
                 file as [competition.photos], or set it in the admin UI under Wettkampf \
                 bearbeiten."
                    .to_string(),
            );
        }
        toml::from_str(content).map_err(|e| e.to_string())
    }

    /// Loads the config file from `path`, or from `./config.toml` if no path is
    /// given and that file exists. Without a file, an empty config is returned.
    /// A `path` that doesn't exist yet counts as an empty config too, so the
    /// first-run setup writes its file exactly there; other read errors fail.
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let path = match path {
            Some(path) => path,
            None if Path::new(DEFAULT_CONFIG_PATH).exists() => Path::new(DEFAULT_CONFIG_PATH),
            None => return Ok(Self::default()),
        };
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(format!("Couldn't read config file {}: {e}", path.display()));
            }
        };
        Self::parse(&content).map_err(|e| format!("Invalid config file {}: {e}", path.display()))
    }
}

/// The competition file: the competition with its categories, finals,
/// timeplan and jury categories. It's only an import; the database is the
/// source of the competition (see `crate::seed::apply`). The export writes
/// one (see `crate::export`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompetitionFile {
    pub competition: CompetitionConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<CategoryConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finals: Vec<FinalConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeplan: Vec<TimeplanEntryConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jury_categories: Vec<JuryCategoryConfig>,
}

impl CompetitionFile {
    pub fn parse(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| e.to_string())
    }

    /// The file as TOML, as `parse` reads it.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Couldn't read competition file {}: {e}", path.display()))?;
        Self::parse(&content)
            .map_err(|e| format!("Invalid competition file {}: {e}", path.display()))
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub base_url: Option<Url>,
    pub http_address: Option<String>,
    pub data_path: Option<PathBuf>,
    pub insecure_cookies: Option<bool>,
    pub reload_db_token: Option<String>,
    pub metrics_token: Option<String>,
    pub local_player: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmtpConfig {
    pub server: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub jwt_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    pub name: String,
    pub email: String,
    pub password: String,
}

impl FromStr for AdminConfig {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompetitionConfig {
    pub id: String,
    pub name: String,
    pub location: String,
    #[serde(deserialize_with = "toml_date", serialize_with = "date_to_toml")]
    pub start_date: Date,
    #[serde(deserialize_with = "toml_date", serialize_with = "date_to_toml")]
    pub end_date: Date,
    #[serde(deserialize_with = "toml_date", serialize_with = "date_to_toml")]
    pub determining_day: Date,
    #[serde(
        deserialize_with = "toml_datetime",
        serialize_with = "datetime_to_toml"
    )]
    pub registration_start: OffsetDateTime,
    #[serde(
        deserialize_with = "toml_datetime",
        serialize_with = "datetime_to_toml"
    )]
    pub registration_end: OffsetDateTime,
    #[serde(
        deserialize_with = "toml_datetime",
        serialize_with = "datetime_to_toml"
    )]
    pub music_upload_end: OffsetDateTime,
    pub fee_per_start: f64,
    /// Formerly `max_fee_per_starter`, which older config files still use.
    #[serde(alias = "max_fee_per_starter", skip_serializing_if = "Option::is_none")]
    pub max_fee_per_rider: Option<f64>,
    /// Where the event takes place, e.g. the hall's name and address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// Shown to clubs for questions, e.g. about overpayments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    /// Link to the official announcement (Ausschreibung).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement_url: Option<String>,
    /// Where the info board's photo QR code leads, e.g. the photo gallery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photos_url: Option<String>,
    #[serde(default, skip_serializing_if = "PaymentConfig::is_empty")]
    pub payment: PaymentConfig,
    /// The clubs' jury obligation; without it there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jury: Option<JuryConfig>,
    /// Features this competition turns on or off; unset ones follow the
    /// build default (see `Features::resolve`).
    #[serde(default, skip_serializing_if = "Features::is_unset")]
    pub features: Features,
    /// Where the info board's photos come from; without it there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photos: Option<PhotosSettings>,
    /// The competition's logo as SVG; raster logos are uploaded in the
    /// admin UI. A replace keeps the logo unless the file sets it.
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub logo_svg: Branding<SvgLogo>,
    /// Instead of `logo_svg`: a URL the server downloads the logo from at
    /// startup and on refresh (#291, ADR 0010).
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub logo_url: Branding<ImageUrl>,
    /// The info board's break image as SVG, like the logo (#291).
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub break_image_svg: Branding<SvgLogo>,
    /// Instead of `break_image_svg`: a URL to download it from.
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub break_image_url: Branding<ImageUrl>,
    /// The sponsor logos of the info board's footer, in order. Left out, a
    /// replace keeps the sponsors; `sponsors = []` removes them (ADR 0010).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sponsors: Option<Vec<SponsorConfig>>,
    /// The primary color of every page, e.g. from the logo, as `#rgb` or
    /// `#rrggbb`; the frontend derives all other colors from it and the
    /// secondary color. A replace keeps it unless the file sets it.
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub primary_color: Branding<HexColor>,
    /// The secondary color, like the primary color.
    #[serde(default, skip_serializing_if = "Branding::is_keep")]
    pub secondary_color: Branding<HexColor>,
}

/// A sponsor of the competition file (#291). The image is an inline `svg` or
/// a `url`; without either, a sponsor of the same name that's already in the
/// database keeps its image (an uploaded raster image can't be in the file),
/// otherwise the sponsor has no image and isn't shown.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SponsorConfig {
    /// The image's alt text and its label in the admin UI.
    pub name: String,
    /// Where the logo leads; the info board doesn't show it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg: Option<SvgLogo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<ImageUrl>,
}

/// A branding setting of the competition file, like the logo (ADR 0009): a
/// replace keeps what the database has unless the file sets it. Left out,
/// it's kept; an empty string resets it to the default built into the
/// binary; any other value sets it, checked by `T`'s `FromStr`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Branding<T> {
    #[default]
    Keep,
    Reset,
    Set(T),
}

impl<T> Branding<T> {
    /// A setting as the admin UI saves it: `None` is the default.
    pub fn set_or_reset(value: Option<T>) -> Self {
        value.map_or(Branding::Reset, Branding::Set)
    }

    pub fn is_keep(&self) -> bool {
        matches!(self, Branding::Keep)
    }

    /// The value it sets; `None` if it keeps or resets it.
    pub fn value(&self) -> Option<&T> {
        match self {
            Branding::Set(value) => Some(value),
            Branding::Keep | Branding::Reset => None,
        }
    }
}

impl<'de, T> Deserialize<'de> for Branding<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() {
            return Ok(Branding::Reset);
        }
        value.parse().map(Branding::Set).map_err(D::Error::custom)
    }
}

impl<T: std::fmt::Display> Serialize for Branding<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Left out of the file by `skip_serializing_if`.
            Branding::Keep => serializer.serialize_none(),
            Branding::Reset => serializer.serialize_str(""),
            Branding::Set(value) => serializer.serialize_str(&value.to_string()),
        }
    }
}

/// Rules of the Jury-Konzept Einrad Freestyle (Stand 15.09.2025), which are
/// the defaults.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JuryConfig {
    /// Minimum age of trained judges on the first competition day.
    #[serde(default = "default_jury_min_age")]
    pub min_age: i64,
    /// Required jury deployments (Einsätze) per started two Küren.
    #[serde(default = "default_deployments_per_two_acts")]
    pub deployments_per_two_acts: i64,
    /// Fee in € per missing deployment.
    #[serde(default = "default_missing_deployment_fee")]
    pub missing_deployment_fee: f64,
    /// End of the judge registration; the registration's end without it.
    #[serde(
        default,
        deserialize_with = "toml_datetime_option",
        serialize_with = "datetime_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub registration_end: Option<OffsetDateTime>,
}

fn default_jury_min_age() -> i64 {
    15
}

fn default_deployments_per_two_acts() -> i64 {
    4
}

fn default_missing_deployment_fee() -> f64 {
    7.0
}

/// Whether `iban` passes the IBAN format and mod-97 check. Spaces and
/// lower case are allowed, as in the config.
pub fn is_valid_iban(iban: &str) -> bool {
    let iban: String = iban
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    if !(15..=34).contains(&iban.len())
        || !iban.chars().all(|c| c.is_ascii_alphanumeric())
        || !iban[..2].chars().all(|c| c.is_ascii_alphabetic())
        || !iban[2..4].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let rearranged = iban[4..].chars().chain(iban[..4].chars());
    let mut remainder = 0u32;
    for c in rearranged {
        let value = c.to_digit(36).expect("alphanumeric");
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

impl CompetitionConfig {
    /// Problems with the payment details that don't stop the server, but
    /// break the payment QR code for clubs.
    pub fn payment_warnings(&self) -> Vec<String> {
        match &self.payment.iban {
            Some(iban) if !is_valid_iban(iban) => vec![format!(
                "[competition.payment] iban {iban:?} is not a valid IBAN; clubs get no payment QR code."
            )],
            _ => vec![],
        }
    }

    /// Checks that the dates are in a sensible order, so a typo in a year or
    /// a swapped date doesn't silently open or close registration.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        let start = self.start_date.midnight().assume_utc();
        if self.start_date > self.end_date {
            errors.push("start_date must not be after end_date");
        }
        if self.name.trim().is_empty() {
            errors.push("name must not be empty");
        }
        if self.location.trim().is_empty() {
            errors.push("location must not be empty");
        }
        if self.fee_per_start < 0.0 || self.max_fee_per_rider.is_some_and(|fee| fee < 0.0) {
            errors.push("fees must not be negative");
        }
        if self.jury.as_ref().is_some_and(|jury| {
            jury.min_age < 0
                || jury.deployments_per_two_acts < 0
                || jury.missing_deployment_fee < 0.0
        }) {
            errors.push("[competition.jury] values must not be negative");
        }
        for (what, svg, url) in [
            (
                "logo",
                self.logo_svg.value().is_some(),
                self.logo_url.value().is_some(),
            ),
            (
                "break_image",
                self.break_image_svg.value().is_some(),
                self.break_image_url.value().is_some(),
            ),
        ] {
            if svg && url {
                return Err(format!(
                    "{what}_svg and {what}_url are both set; a {what} comes from one of them"
                ));
            }
        }
        let mut names = std::collections::HashSet::new();
        for sponsor in self.sponsors.iter().flatten() {
            if !names.insert(sponsor.name.trim().to_lowercase()) {
                return Err(format!("The sponsor {} is listed twice", sponsor.name));
            }
            if sponsor.svg.is_some() && sponsor.url.is_some() {
                return Err(format!(
                    "The sponsor {} has both svg and url; its image comes from one of them",
                    sponsor.name
                ));
            }
            images::clean_name(&sponsor.name).map_err(|e| format!("sponsors: {e}"))?;
            images::clean_link(sponsor.link.as_deref())
                .map_err(|e| format!("sponsor {}: {e}", sponsor.name))?;
        }
        if self.registration_start >= self.registration_end {
            errors.push("registration_start must be before registration_end");
        }
        if self.registration_end > start {
            errors.push("registration_end must not be after start_date");
        }
        if self.music_upload_end > start {
            errors.push("music_upload_end must not be after start_date");
        }
        if let Some(end) = self.jury.as_ref().and_then(|jury| jury.registration_end)
            && end > start
        {
            errors.push("[competition.jury] registration_end must not be after start_date");
        }
        if !errors.is_empty() {
            return Err(format!("Invalid [competition]:\n- {}", errors.join("\n- ")));
        }
        if let Some(photos) = &self.photos {
            photos
                .check()
                .map_err(|e| format!("Invalid [competition.photos]: {e}"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PaymentConfig {
    #[serde(
        deserialize_with = "toml_date_option",
        serialize_with = "date_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub deadline: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iban: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl PaymentConfig {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub act_type: ActType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub single_male: bool,
    /// Minimum age threshold; birthdays on the determining day remain younger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_age: Option<f64>,
    /// Upper age threshold, e.g. 15 for "U15"; birthdays on the determining day remain younger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub under_age: Option<f64>,
    /// Explicit lower birthday bound (inclusive); overrides `under_age`.
    #[serde(
        default,
        deserialize_with = "toml_date_option",
        serialize_with = "date_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub from_birthday: Option<Date>,
    /// Explicit upper birthday bound (exclusive); overrides `min_age`.
    #[serde(
        default,
        deserialize_with = "toml_date_option",
        serialize_with = "date_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub to_birthday: Option<Date>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub einfahrzeit_minutes: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub act_duration_minutes: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub judge_duration_minutes: f64,
}

/// A final: reserved slots for the best riders of the heats, e.g. 5 slots
/// "U11–U15", planned before the results are known (see `crate::finals`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FinalConfig {
    /// Unique; `[[timeplan]] final = …` refers to it.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub act_type: ActType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub single_male: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub einfahrzeit_minutes: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub act_duration_minutes: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub judge_duration_minutes: f64,
    /// The slots in start order, as groups like `{ label = "U11–U15", count = 5 }`.
    pub slots: Vec<crate::finals::SlotGroup>,
    /// The categories whose acts can be placed in the slots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// What judges register for and get assigned to, e.g. one heat block (see
/// `crate::jury`). Only seeded into an empty table.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JuryCategoryConfig {
    pub name: String,
    /// Jury deployments (Einsätze) it counts for; a heat counts 2, a final
    /// or group 1.
    #[serde(default = "default_category_deployments")]
    pub deployments: i64,
    /// The day it runs; within the event.
    #[serde(
        default,
        deserialize_with = "toml_date_option",
        serialize_with = "date_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub day: Option<Date>,
    /// Judges it needs per task. 5 allows dropping the highest and lowest
    /// score (Konzept Gemeinsame Wettkämpfe 4.1).
    #[serde(default = "default_needed_judges")]
    pub needed_technical: i64,
    #[serde(default = "default_needed_judges")]
    pub needed_performance: i64,
    #[serde(default = "default_needed_dismount_counters")]
    pub needed_dismounts: i64,
}

fn default_category_deployments() -> i64 {
    2
}

fn default_needed_judges() -> i64 {
    5
}

fn default_needed_dismount_counters() -> i64 {
    3
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeplanEntryConfig {
    /// `"acts"` for a start block with a label, e.g. "Nachstarter" (with or
    /// without a category or final). Without it, entries with a category or
    /// final are start blocks, the others custom entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<EntryKind>,
    /// Name of the category this entry runs. Leave empty for custom entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Name of the final this entry runs, instead of a category.
    #[serde(rename = "final", skip_serializing_if = "Option::is_none")]
    pub final_name: Option<String>,
    /// Label of a custom entry, e.g. "Mittagspause", or of a start block
    /// (`kind = "acts"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Duration of a custom entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<f64>,
    /// Start blocks: the warm-up time (Einfahrzeit) before the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub einfahrzeit_minutes: Option<f64>,
    /// Custom entries: photos next to the clock on the info board.
    #[serde(
        default = "crate::utils::default_true",
        skip_serializing_if = "is_true"
    )]
    pub show_photos: bool,
    /// Custom entries: an internal timeplan entry, only for the organizer
    /// team.
    #[serde(default, skip_serializing_if = "is_false")]
    pub internal: bool,
    #[serde(
        default,
        deserialize_with = "toml_datetime_option",
        serialize_with = "datetime_option_to_toml",
        skip_serializing_if = "Option::is_none"
    )]
    pub earliest_start: Option<OffsetDateTime>,
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

fn parse_date(value: &str) -> Result<Date, String> {
    Date::parse(value, format_description!("[year]-[month]-[day]"))
        .map_err(|e| format!("expected a date like 2027-06-01, got {value}: {e}"))
}

fn parse_datetime(value: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|e| {
        format!(
            "expected a date and time with offset like 2027-06-05T10:00:00+02:00, got {value}: {e}"
        )
    })
}

pub(crate) fn toml_date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
    let value = toml::value::Datetime::deserialize(deserializer)?;
    parse_date(&value.to_string()).map_err(D::Error::custom)
}

pub(crate) fn toml_date_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Date>, D::Error> {
    toml_date(deserializer).map(Some)
}

pub(crate) fn toml_datetime<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<OffsetDateTime, D::Error> {
    let value = toml::value::Datetime::deserialize(deserializer)?;
    parse_datetime(&value.to_string()).map_err(D::Error::custom)
}

pub(crate) fn toml_datetime_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<OffsetDateTime>, D::Error> {
    toml_datetime(deserializer).map(Some)
}

/// A date as a TOML date, e.g. `2027-06-05`, not as a string.
fn date_to_toml<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
    toml::value::Datetime::from_str(&date.to_string())
        .map_err(serde::ser::Error::custom)?
        .serialize(serializer)
}

fn date_option_to_toml<S: Serializer>(
    date: &Option<Date>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match date {
        Some(date) => date_to_toml(date, serializer),
        None => serializer.serialize_none(),
    }
}

/// A date and time as a TOML datetime with its offset, e.g.
/// `2027-06-05T10:00:00+02:00`.
fn datetime_to_toml<S: Serializer>(
    datetime: &OffsetDateTime,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let text = datetime
        .format(&Rfc3339)
        .map_err(serde::ser::Error::custom)?;
    toml::value::Datetime::from_str(&text)
        .map_err(serde::ser::Error::custom)?
        .serialize(serializer)
}

fn datetime_option_to_toml<S: Serializer>(
    datetime: &Option<OffsetDateTime>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match datetime {
        Some(datetime) => datetime_to_toml(datetime, serializer),
        None => serializer.serialize_none(),
    }
}

/// Fully resolved settings the server runs with.
#[derive(Debug)]
pub struct Settings {
    pub base_url: Url,
    pub http_address: String,
    pub smtp_server: String,
    pub smtp_username: String,
    pub smtp_password: String,
    pub database_url: String,
    pub data_path: PathBuf,
    pub jwt_secret: String,
    pub admin: Option<AdminConfig>,
    pub insecure_cookies: bool,
    /// Lets a logged-in non-admin reload the database; `None` turns that off.
    pub reload_db_token: Option<String>,
    pub metrics_token: Option<String>,
    pub local_player: Option<String>,
    /// The competition file, if one is given or `./competition.toml` exists.
    pub competition_file: Option<PathBuf>,
    /// Whether the competition file replaces the database's competition.
    pub replace_competition: bool,
    /// Problems that don't stop the server, e.g. insecure defaults.
    pub warnings: Vec<String>,
}

/// What the server needs before it knows whether the system config is
/// complete: where to listen, which database to open and the configured
/// base URL. First-run setup runs on these (see `crate::setup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bootstrap {
    /// The base URL from CLI, environment or file; `None` if none is set,
    /// unlike [`Settings::base_url`], which falls back to the local default.
    pub configured_base_url: Option<Url>,
    pub http_address: String,
    pub database_url: String,
    pub data_path: PathBuf,
}

impl Bootstrap {
    /// Resolves the values that have defaults, so it works for an incomplete
    /// config too.
    #[must_use]
    pub fn resolve(cli: &CliArgs, file: &ConfigFile) -> Self {
        Self {
            configured_base_url: cli
                .base_url
                .clone()
                .or_else(|| file.server.base_url.clone()),
            http_address: cli
                .http_address
                .clone()
                .or_else(|| file.server.http_address.clone())
                .unwrap_or_else(|| DEFAULT_HTTP_ADDRESS.to_string()),
            database_url: cli
                .db
                .clone()
                .or_else(|| file.database.url.clone())
                .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_string()),
            data_path: cli
                .data
                .clone()
                .or_else(|| file.server.data_path.clone())
                .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_PATH)),
        }
    }
}

/// The competition file: `--competition`/`COMPETITION`, or `./competition.toml`
/// if it exists.
#[must_use]
pub fn competition_file_path(cli: &CliArgs) -> Option<PathBuf> {
    cli.competition.clone().or_else(|| {
        Path::new(DEFAULT_COMPETITION_PATH)
            .exists()
            .then(|| PathBuf::from(DEFAULT_COMPETITION_PATH))
    })
}

/// A setting that is empty or only whitespace counts as not set, so an empty
/// `JWT_SECRET=` neither passes as a secret nor shadows the file's value.
pub(crate) fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

impl Settings {
    /// Merges CLI/env values, the config file and the defaults. Fails with one message listing every missing required value.
    ///
    /// ```
    /// use einrad_server::config::{CliArgs, ConfigFile, Settings};
    ///
    /// let file = ConfigFile::parse(r#"
    ///     [smtp]
    ///     server = "smtp.example.org"
    ///     user = "cup@example.org"
    ///     password = "from-file"
    ///
    ///     [auth]
    ///     jwt_secret = "a-long-random-secret"
    /// "#).unwrap();
    /// let cli = CliArgs { smtp_password: Some("from-env".into()), ..Default::default() };
    ///
    /// let settings = Settings::resolve(cli, file).unwrap();
    /// assert_eq!(settings.smtp_password, "from-env");
    /// assert_eq!(settings.smtp_server, "smtp.example.org");
    /// assert_eq!(settings.http_address, "127.0.0.1:3000");
    /// // jwt_secret is required; reload_db_token is optional (off without it).
    /// assert!(settings.reload_db_token.is_none());
    /// assert!(Settings::resolve(CliArgs::default(), ConfigFile::parse("").unwrap()).is_err());
    /// ```
    pub fn resolve(cli: CliArgs, file: ConfigFile) -> Result<Self, String> {
        let bootstrap = Bootstrap::resolve(&cli, &file);
        let competition_file = competition_file_path(&cli);
        let mut missing = Vec::new();
        let mut required = |value: Option<String>, name: &str| {
            value.unwrap_or_else(|| {
                missing.push(name.to_string());
                String::new()
            })
        };

        let smtp_server = required(
            non_blank(cli.smtp_server).or(non_blank(file.smtp.server)),
            "SMTP server (--smtp-server, SMTP_SERVER or [smtp] server)",
        );
        let smtp_username = required(
            non_blank(cli.smtp_username).or(non_blank(file.smtp.user)),
            "SMTP user (--smtp-username, SMTP_USER or [smtp] user)",
        );
        let smtp_password = required(
            non_blank(cli.smtp_password).or(non_blank(file.smtp.password)),
            "SMTP password (--smtp-password, SMTP_PASSWORD or [smtp] password)",
        );
        let jwt_secret = required(
            non_blank(cli.jwt_secret).or(non_blank(file.auth.jwt_secret)),
            "JWT secret (--jwt-secret, JWT_SECRET or [auth] jwt_secret)",
        );
        if !missing.is_empty() {
            return Err(format!(
                "Missing required settings:\n- {}",
                missing.join("\n- ")
            ));
        }

        let reload_db_token =
            non_blank(cli.reload_db_token).or(non_blank(file.server.reload_db_token));

        Ok(Self {
            base_url: bootstrap
                .configured_base_url
                .unwrap_or_else(|| Url::parse(DEFAULT_BASE_URL).expect("valid default URL")),
            http_address: bootstrap.http_address,
            smtp_server,
            smtp_username,
            smtp_password,
            database_url: bootstrap.database_url,
            data_path: bootstrap.data_path,
            jwt_secret,
            admin: cli.admin.or(file.admin),
            insecure_cookies: cli.insecure_cookies || file.server.insecure_cookies.unwrap_or(false),
            reload_db_token,
            metrics_token: cli
                .metrics_token
                .or(file.server.metrics_token)
                .filter(|token| !token.is_empty()),
            local_player: cli
                .local_player
                .or(file.server.local_player)
                .filter(|name| !name.trim().is_empty()),
            competition_file,
            replace_competition: cli.replace_competition,
            warnings: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMTP: &str = r#"
        [smtp]
        server = "smtp.example.org"
        user = "cup@example.org"
        password = "secret"

        [auth]
        jwt_secret = "a-real-secret"
    "#;

    fn resolve(cli: CliArgs, file: &str) -> Result<Settings, String> {
        Settings::resolve(cli, ConfigFile::parse(file).unwrap())
    }

    #[test]
    fn cli_and_env_take_precedence_over_file() {
        let file = format!(
            "{SMTP}\n[server]\nhttp_address = \"0.0.0.0:1\"\n[database]\nurl = \"sqlite://file.sqlite\""
        );
        let cli = CliArgs {
            http_address: Some("0.0.0.0:2".into()),
            ..Default::default()
        };
        let settings = resolve(cli, &file).unwrap();
        assert_eq!(settings.http_address, "0.0.0.0:2");
        assert_eq!(settings.database_url, "sqlite://file.sqlite");
    }

    #[test]
    fn bootstrap_resolves_without_a_complete_config() {
        let file = ConfigFile::parse("[server]\nbase_url = \"https://cup.example.org\"").unwrap();
        let cli = CliArgs {
            http_address: Some("0.0.0.0:2".into()),
            ..Default::default()
        };
        let bootstrap = Bootstrap::resolve(&cli, &file);
        assert_eq!(
            bootstrap.configured_base_url.unwrap().as_str(),
            "https://cup.example.org/"
        );
        assert_eq!(bootstrap.http_address, "0.0.0.0:2");
        assert_eq!(bootstrap.database_url, DEFAULT_DATABASE_URL);
        assert_eq!(bootstrap.data_path, PathBuf::from(DEFAULT_DATA_PATH));
    }

    #[test]
    fn bootstrap_has_no_base_url_unless_one_is_configured() {
        let bootstrap = Bootstrap::resolve(&CliArgs::default(), &ConfigFile::default());
        assert_eq!(bootstrap.configured_base_url, None);
    }

    #[test]
    fn a_blank_smtp_setting_counts_as_missing() {
        let cli = CliArgs {
            smtp_server: Some("  ".into()),
            ..Default::default()
        };
        let err = resolve(cli, "[auth]\njwt_secret = \"s\"").unwrap_err();
        assert!(err.contains("SMTP server"), "{err}");
    }

    #[test]
    fn defaults_apply_without_values() {
        let settings = resolve(CliArgs::default(), SMTP).unwrap();
        assert_eq!(settings.base_url.as_str(), "http://localhost:3000/");
        assert_eq!(settings.database_url, DEFAULT_DATABASE_URL);
        assert_eq!(settings.data_path, PathBuf::from(DEFAULT_DATA_PATH));
        assert!(!settings.insecure_cookies);
        assert!(!settings.replace_competition);
    }

    #[test]
    fn all_missing_required_values_are_reported() {
        let err = resolve(CliArgs::default(), "").unwrap_err();
        assert!(err.contains("SMTP server"), "{err}");
        assert!(err.contains("SMTP user"), "{err}");
        assert!(err.contains("SMTP password"), "{err}");
        assert!(err.contains("JWT_SECRET"), "{err}");
    }

    #[test]
    fn jwt_secret_is_required() {
        let smtp_only = r#"
            [smtp]
            server = "smtp.example.org"
            user = "cup@example.org"
            password = "secret"
        "#;
        let err = resolve(CliArgs::default(), smtp_only).unwrap_err();
        assert!(err.contains("--jwt-secret"), "{err}");
        assert!(err.contains("JWT_SECRET"), "{err}");
        assert!(err.contains("[auth] jwt_secret"), "{err}");

        let cli = CliArgs {
            jwt_secret: Some("from-env".into()),
            ..Default::default()
        };
        let settings = resolve(cli, smtp_only).unwrap();
        assert_eq!(settings.jwt_secret, "from-env");

        // Empty or blank doesn't count as set, and doesn't shadow the file.
        for blank in ["", "  "] {
            let cli = CliArgs {
                jwt_secret: Some(blank.into()),
                ..Default::default()
            };
            let err = resolve(cli, smtp_only).unwrap_err();
            assert!(err.contains("JWT_SECRET"), "{err}");
        }
        let cli = CliArgs {
            jwt_secret: Some(String::new()),
            ..Default::default()
        };
        let settings = resolve(cli, SMTP).unwrap();
        assert_eq!(settings.jwt_secret, "a-real-secret");
    }

    #[test]
    fn reload_db_token_is_optional_and_empty_means_off() {
        let settings = resolve(CliArgs::default(), SMTP).unwrap();
        assert_eq!(settings.reload_db_token, None);

        let cli = CliArgs {
            reload_db_token: Some("  ".into()),
            ..Default::default()
        };
        assert_eq!(resolve(cli, SMTP).unwrap().reload_db_token, None);

        let cli = CliArgs {
            reload_db_token: Some("a-real-token".into()),
            ..Default::default()
        };
        let settings = resolve(cli, SMTP).unwrap();
        assert_eq!(settings.reload_db_token.as_deref(), Some("a-real-token"));
    }

    #[test]
    fn admin_can_come_from_json_or_file() {
        let admin: AdminConfig = r#"{"name":"A","email":"a@b.c","password":"p"}"#.parse().unwrap();
        assert_eq!(admin.name, "A");

        let file = format!("{SMTP}\n[admin]\nname = \"B\"\nemail = \"b@c.d\"\npassword = \"q\"");
        let settings = resolve(CliArgs::default(), &file).unwrap();
        assert_eq!(settings.admin.unwrap().name, "B");
    }

    #[test]
    fn competition_dates_are_parsed() {
        let file = CompetitionFile::parse(
            r#"
            [competition]
            id = "cup-2027"
            name = "Cup 2027"
            location = "Halle"
            start_date = 2027-06-05
            end_date = 2027-06-06
            determining_day = 2027-06-01
            registration_start = 2026-11-01T00:00:00+01:00
            registration_end = 2027-04-30T23:59:59+02:00
            music_upload_end = 2027-05-20T23:59:59+02:00
            fee_per_start = 15
            max_fee_per_rider = 45

            [competition.payment]
            deadline = 2027-05-15
            iban = "DE00"
            "#,
        )
        .unwrap();
        let competition = file.competition;
        assert_eq!(competition.determining_day.to_string(), "2027-06-01");
        assert_eq!(competition.registration_start.offset().whole_hours(), 1);
        assert_eq!(competition.fee_per_start, 15.0);
        assert_eq!(
            competition.payment.deadline.unwrap().to_string(),
            "2027-05-15"
        );
        assert_eq!(competition.payment.bic, None);
    }

    fn competition(dates: &str) -> CompetitionConfig {
        let file = CompetitionFile::parse(&format!(
            "[competition]\nid = \"c\"\nname = \"C\"\nlocation = \"L\"\nfee_per_start = 15\n{dates}"
        ))
        .unwrap();
        file.competition
    }

    const VALID_DATES: &str = "start_date = 2027-02-12
end_date = 2027-02-14
determining_day = 2027-06-01
registration_start = 2026-11-20T00:00:00+01:00
registration_end = 2026-12-20T23:59:59+01:00
music_upload_end = 2027-01-20T23:59:59+01:00";

    #[test]
    fn features_in_the_system_config_are_rejected_with_where_they_went() {
        let err = ConfigFile::parse(&format!("{SMTP}\n[features]\njury_registration = true"))
            .unwrap_err();
        assert!(err.contains("[competition.features]"), "{err}");
    }

    #[test]
    fn photos_in_the_system_config_are_rejected_with_where_they_went() {
        let err = ConfigFile::parse(&format!("{SMTP}\n[photos]\nsource = \"/fotos\"")).unwrap_err();
        assert!(err.contains("[competition.photos]"), "{err}");
    }

    #[test]
    fn features_and_photos_are_competition_settings() {
        let set = competition(&format!(
            "{VALID_DATES}\n[competition.features]\njury_registration = true\nmoderation_input = false\n\
             [competition.photos]\nsource = \"https://cloud.example.org/s/AbC123\"\npassword = \"p\""
        ));
        assert_eq!(set.features.jury_registration, Some(true));
        assert_eq!(set.features.moderation_input, Some(false));
        let photos = set.photos.as_ref().unwrap();
        assert_eq!(photos.source, "https://cloud.example.org/s/AbC123");
        assert_eq!(photos.user, None);
        assert_eq!(photos.password.as_deref(), Some("p"));
        set.validate().unwrap();

        let unset = competition(VALID_DATES);
        assert_eq!(unset.features, Features::default());
        assert_eq!(unset.photos, None);

        let invalid = competition(&format!(
            "{VALID_DATES}\n[competition.photos]\nsource = \"https://cloud.example.org/s/\""
        ));
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .contains("[competition.photos]")
        );
    }

    #[test]
    fn jury_rules_default_to_the_jury_konzept() {
        assert!(competition(VALID_DATES).jury.is_none());

        let jury = competition(&format!("{VALID_DATES}\n[competition.jury]"))
            .jury
            .unwrap();
        assert_eq!(jury.min_age, 15);
        assert_eq!(jury.deployments_per_two_acts, 4);
        assert_eq!(jury.missing_deployment_fee, 7.0);
        assert_eq!(jury.registration_end, None);

        let jury = competition(&format!(
            "{VALID_DATES}\n[competition.jury]\nmin_age = 16\nregistration_end = 2027-01-10T23:59:59+01:00"
        ))
        .jury
        .unwrap();
        assert_eq!(jury.min_age, 16);
        assert_eq!(
            jury.registration_end.unwrap().format(&Rfc3339).unwrap(),
            "2027-01-10T23:59:59+01:00"
        );

        let late = competition(&format!(
            "{VALID_DATES}\n[competition.jury]\nregistration_end = 2027-02-13T00:00:00+01:00"
        ));
        assert!(late.validate().unwrap_err().contains("[competition.jury]"));
    }

    #[test]
    fn ibans_are_checked() {
        assert!(is_valid_iban("DE02 1203 0000 0000 2020 51"));
        assert!(is_valid_iban("de02120300000000202051"));
        assert!(!is_valid_iban("DE03 1203 0000 0000 2020 51"));
        assert!(!is_valid_iban("DE00"));
    }

    #[test]
    fn valid_competition_dates_pass() {
        competition(VALID_DATES).validate().unwrap();
    }

    #[test]
    fn invalid_competition_dates_are_reported() {
        let cases = [
            (
                "end_date = 2027-02-14",
                "end_date = 2027-02-11",
                "start_date must not be after end_date",
            ),
            (
                "registration_start = 2026-11-20T00:00:00+01:00",
                "registration_start = 2026-12-21T00:00:00+01:00",
                "registration_start must be before registration_end",
            ),
            (
                "registration_end = 2026-12-20T23:59:59+01:00",
                "registration_end = 2027-12-20T23:59:59+01:00",
                "registration_end must not be after start_date",
            ),
            (
                "music_upload_end = 2027-01-20T23:59:59+01:00",
                "music_upload_end = 2027-02-20T23:59:59+01:00",
                "music_upload_end must not be after start_date",
            ),
        ];
        for (from, to, expected) in cases {
            let err = competition(&VALID_DATES.replace(from, to))
                .validate()
                .unwrap_err();
            assert!(err.contains(expected), "{to}: {err}");
        }
    }

    #[test]
    fn datetime_without_offset_is_rejected() {
        let err = CompetitionFile::parse(&format!(
            "[competition]\nid = \"c\"\nname = \"C\"\nlocation = \"L\"\nfee_per_start = 15\n{VALID_DATES}\n\
             [[timeplan]]\ncategory = \"NEM\"\nearliest_start = 2027-06-05T10:00:00"
        ))
        .unwrap_err();
        assert!(err.contains("offset"), "{err}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = ConfigFile::parse("[server]\nbase_ulr = \"http://x\"").unwrap_err();
        assert!(err.contains("base_ulr"), "{err}");
    }

    #[test]
    fn a_combined_config_is_rejected_with_how_to_split_it() {
        let err = ConfigFile::parse(&format!(
            "{SMTP}\n[competition]\nid = \"c\"\n[[categories]]\nname = \"A\""
        ))
        .unwrap_err();
        assert!(err.contains("[competition], [categories]"), "{err}");
        assert!(err.contains("--competition"), "{err}");
        assert!(err.contains("competition.example.toml"), "{err}");
    }

    #[test]
    fn example_config_is_valid() {
        let content =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config.example.toml"))
                .unwrap();
        ConfigFile::parse(&content).unwrap();
    }

    #[test]
    fn example_competition_is_valid() {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/competition.example.toml"
        ))
        .unwrap();
        let file = CompetitionFile::parse(&content).unwrap();
        let competition = &file.competition;
        competition.validate().unwrap();
        let categories =
            crate::seed::resolve_categories(&file.categories, competition.determining_day).unwrap();
        let warnings = crate::seed::coverage_warnings(&categories);
        assert!(warnings.is_empty(), "{warnings:?}");
        let warnings = crate::seed::missing_act_type_warnings(&categories);
        assert!(warnings.is_empty(), "{warnings:?}");
        let category_names = file
            .categories
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        let finals = crate::seed::resolve_finals(&file.finals).unwrap();
        let final_names = finals.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
        crate::seed::validate_timeplan(&file.timeplan, &category_names, &final_names).unwrap();
        let warnings = crate::seed::unscheduled_category_warnings(&file.timeplan, &category_names);
        assert!(warnings.is_empty(), "{warnings:?}");
        let warnings = crate::seed::unscheduled_final_warnings(&file.timeplan, &final_names);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
