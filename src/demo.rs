//! Demo data for development and tests.
//!
//! A demo file (see `demo-data.toml`) describes a complete event: the
//! competition, categories and timeplan in the same format as the competition
//! file, plus users, clubs, riders, groups and act details. [`DemoData::apply`]
//! writes it into an empty, migrated database; the `seed_demo` binary
//! recreates the database before doing so.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::ids::{ActId, ClubId, JuryUnitId, RiderId, UserId};
use db_core::{
    Db, DbError,
    riders::Pair,
    seed::{SeedActDetails, SeedClub, SeedGroup, SeedRider, SeedUser},
};
use password_auth::generate_hash;
use serde::Deserialize;
use time::{Date, OffsetDateTime, UtcOffset};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    act_type::ActType,
    config::{
        CategoryConfig, CompetitionConfig, CompetitionFile, FinalConfig, HeatConfig,
        TimeplanEntryConfig, toml_date, toml_datetime_option,
    },
    gender::Gender,
    groups, seed,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoData {
    pub competition: CompetitionConfig,
    #[serde(default)]
    pub categories: Vec<CategoryConfig>,
    #[serde(default)]
    pub heats: Vec<HeatConfig>,
    #[serde(default)]
    pub finals: Vec<FinalConfig>,
    #[serde(default)]
    pub timeplan: Vec<DemoTimeplanEntry>,
    #[serde(default)]
    pub users: Vec<DemoUser>,
    #[serde(default)]
    pub clubs: Vec<DemoClub>,
    #[serde(default)]
    pub groups: Vec<DemoGroup>,
    #[serde(default)]
    pub acts: Vec<DemoAct>,
    /// Directory that relative `song_source` paths are resolved against.
    #[serde(skip)]
    pub base_dir: PathBuf,
}

/// A timeplan entry as in the config, plus its live state.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoTimeplanEntry {
    pub category: Option<String>,
    #[serde(rename = "final")]
    pub final_name: Option<String>,
    pub label: Option<String>,
    pub duration_minutes: Option<f64>,
    #[serde(default = "crate::utils::default_true")]
    pub show_photos: bool,
    #[serde(default)]
    pub internal: bool,
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub earliest_start: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub ended_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoUser {
    pub name: String,
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub roles: Vec<crate::roles::Role>,
    #[serde(default = "crate::utils::default_true")]
    pub email_verified: bool,
    /// Pending email verification, usable at `/verify_email?token=…`.
    pub verification_token: Option<Uuid>,
    /// Pending password reset, usable at `/reset_password?token=…`.
    pub password_reset_token: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoClub {
    pub name: String,
    /// Name of the owning user.
    pub owner: String,
    /// Amount in € the club has paid, as one payment.
    pub payment: Option<f64>,
    /// Waiver reason: all the club's fees are waived.
    pub fee_waiver: Option<String>,
    #[serde(default)]
    pub riders: Vec<DemoRider>,
    #[serde(default)]
    pub judges: Vec<DemoJudge>,
}

/// A judge of a demo club (jury registration).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoJudge {
    pub firstname: String,
    pub lastname: String,
    #[serde(deserialize_with = "toml_date")]
    pub birthdate: Date,
    #[serde(default = "crate::utils::default_true")]
    pub trained: bool,
    pub max_deployments: Option<i64>,
    pub note: Option<String>,
    /// Jury units (`heat: <name>` or `final: <name>`) and tasks.
    #[serde(default)]
    pub registrations: Vec<DemoJudgeRegistration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoJudgeRegistration {
    pub unit: String,
    pub task: crate::jury::JudgeTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Single {
    Male,
    Female,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoRider {
    pub firstname: String,
    pub lastname: String,
    #[serde(deserialize_with = "toml_date")]
    pub birthdate: Date,
    pub single: Option<Single>,
    /// Defaults to the single's.
    pub gender: Option<Single>,
    #[serde(default)]
    pub pair: bool,
    /// Full name of a registered pair partner, in any club. The partner is
    /// linked back automatically.
    pub partner: Option<String>,
    /// Name of a pair partner that isn't registered (yet).
    pub partner_name: Option<String>,
    /// When the rider checked in on competition day.
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub checked_in_at: Option<OffsetDateTime>,
    /// Waiver reason: all the rider's starts are free.
    pub fee_waiver: Option<String>,
}

impl DemoRider {
    fn full_name(&self) -> String {
        format!("{} {}", self.firstname, self.lastname)
    }
}

/// A group act (Kleingruppe / Großgruppe) with riders from any club. Like in
/// the app, its type follows from the number of regular riders and a rider can
/// be in only one group of each type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoGroup {
    /// The group's name, unique across the competition.
    pub name: String,
    /// Name of the club that created the group.
    pub owner: String,
    /// `group_small` (Kleingruppe) or `group_large` (Großgruppe), as the
    /// owner club chose it.
    pub act_type: ActType,
    /// Full names of the regular riders.
    #[serde(default)]
    pub members: Vec<String>,
    /// Full names of the replacement riders (Ersatzfahrer).
    #[serde(default)]
    pub replacements: Vec<String>,
}

/// Details of an act. Acts themselves are created for every single rider,
/// linked pair and group; this selects one, either by a participant and the
/// act type or by the group name, and sets its fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoAct {
    /// Full name of a participant, together with `act_type`.
    pub rider: Option<String>,
    pub act_type: Option<ActType>,
    /// Name of a group, instead of `rider` and `act_type`.
    pub group: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    /// File name of the song as shown in the UI. Without `song_source`, a short
    /// generated tone is stored.
    pub song: Option<String>,
    /// Audio file to store as the song, relative to the demo file.
    pub song_source: Option<PathBuf>,
    /// Where to download the song from, if there is no `song_source` (see
    /// [`DemoData::fetch_songs`]). Without the download, the tone is stored.
    pub song_url: Option<String>,
    #[serde(default)]
    pub song_checked: bool,
    pub order: Option<i64>,
    /// Waiver reason: the act's start is free for every rider.
    pub fee_waiver: Option<String>,
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "toml_datetime_option")]
    pub ended_at: Option<OffsetDateTime>,
}

impl DemoData {
    pub fn parse(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Couldn't read demo file {}: {e}", path.display()))?;
        let mut data = Self::parse(&content)
            .map_err(|e| format!("Invalid demo file {}: {e}", path.display()))?;
        data.base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(data)
    }

    /// Downloads the songs with a `song_url` into `cache`, unless they are
    /// there already, and uses them as `song_source`. Songs that can't be
    /// downloaded (e.g. offline or without curl) stay generated tones.
    pub fn fetch_songs(&mut self, cache: &Path) -> Result<(), String> {
        std::fs::create_dir_all(cache)
            .map_err(|e| format!("Couldn't create song cache {}: {e}", cache.display()))?;
        // `apply` resolves relative sources against the demo file's directory.
        let cache = std::path::absolute(cache)
            .map_err(|e| format!("Couldn't resolve song cache {}: {e}", cache.display()))?;
        for act in &mut self.acts {
            let Some(url) = act.song_url.as_deref() else {
                continue;
            };
            if act.song_source.is_some() {
                continue;
            }
            let file = url::Url::parse(url)
                .ok()
                .and_then(|url| Some(url.path_segments()?.next_back()?.to_string()))
                .filter(|file| !file.is_empty())
                .ok_or_else(|| format!("song_url {url} has no file name"))?;
            let path = cache.join(file);
            if !path.exists() {
                info!("Downloading {url}");
                if let Err(e) = download(url, &path) {
                    warn!("Couldn't download {url}, using a tone instead: {e}");
                    continue;
                }
            }
            act.song_source = Some(path);
        }
        Ok(())
    }

    /// Writes the demo data into an empty, migrated database. Song files are
    /// stored in `data_path`.
    pub async fn apply(&self, repo: &dyn Db, data_path: &Path) -> Result<(), String> {
        let db_error = |e: DbError| format!("Database error while seeding demo data: {e}");

        let timeplan: Vec<TimeplanEntryConfig> = self
            .timeplan
            .iter()
            .map(|entry| TimeplanEntryConfig {
                kind: None,
                category: entry.category.clone(),
                final_name: entry.final_name.clone(),
                label: entry.label.clone(),
                duration_minutes: entry.duration_minutes,
                einfahrzeit_minutes: None,
                show_photos: entry.show_photos,
                internal: entry.internal,
                earliest_start: entry.earliest_start,
            })
            .collect();
        let file = CompetitionFile {
            competition: self.competition.clone(),
            categories: self.categories.clone(),
            heats: self.heats.clone(),
            finals: self.finals.clone(),
            timeplan,
        };
        seed::apply(repo, Some(&file), false, data_path).await?;

        let progress: Vec<_> = self
            .timeplan
            .iter()
            .map(|entry| (entry.started_at.map(utc), entry.ended_at.map(utc)))
            .collect();
        repo.seed_timeplan_progress(&progress)
            .await
            .map_err(db_error)?;

        let users = self.insert_users(repo).await?;
        let riders = self.insert_clubs(repo, &users).await?;
        self.register_starts(repo, &riders).await?;
        self.insert_judges(repo).await?;

        let group_ids = self.insert_groups(repo, &riders).await?;
        self.update_acts(repo, &riders, &group_ids, data_path)
            .await?;

        info!(
            "Seeded {} users, {} clubs, {} riders and {} groups from the demo data",
            self.users.len(),
            self.clubs.len(),
            riders.len(),
            group_ids.len()
        );
        Ok(())
    }

    async fn insert_users(&self, repo: &dyn Db) -> Result<HashMap<String, UserId>, String> {
        let now = OffsetDateTime::now_utc();
        // Hashing is slow, so users sharing a password share its hash.
        let mut hashes: HashMap<&str, String> = HashMap::new();
        let mut ids = HashMap::new();
        for user in &self.users {
            let password = user.password.as_str();
            let hash = hashes
                .entry(password)
                .or_insert_with(|| generate_hash(password))
                .clone();
            let id = UserId::new();
            repo.seed_user(&SeedUser {
                id,
                email: user.email.clone(),
                email_verified: user.email_verified,
                name: user.name.clone(),
                password_hash: hash,
                roles: user.roles.clone(),
                verification_token: user.verification_token,
                password_reset_token: user.password_reset_token,
                now,
            })
            .await
            .map_err(|e| format!("Couldn't insert user {}: {e}", user.name))?;
            ids.insert(user.name.clone(), id);
        }
        Ok(ids)
    }

    /// Inserts the clubs and their riders, without partner links. Returns the
    /// riders by full name.
    async fn insert_clubs(
        &self,
        repo: &dyn Db,
        users: &HashMap<String, UserId>,
    ) -> Result<RiderIds<'_>, String> {
        let mut riders = RiderIds::default();
        for club in &self.clubs {
            let owner_id = *users.get(&club.owner).ok_or_else(|| {
                format!(
                    "Club {}: owner {} is not a demo user.",
                    club.name, club.owner
                )
            })?;
            let club_id = ClubId::new();
            repo.seed_club(&SeedClub {
                id: club_id,
                name: club.name.clone(),
                owner: owner_id,
                fee_waiver: club.fee_waiver.clone(),
                payment: club.payment,
                payment_date: OffsetDateTime::now_utc().date(),
            })
            .await
            .map_err(|e| format!("Couldn't insert club {}: {e}", club.name))?;

            for rider in &club.riders {
                let name = rider.full_name();
                if !rider.pair && (rider.partner.is_some() || rider.partner_name.is_some()) {
                    return Err(format!("Rider {name} has a partner but pair = false."));
                }
                if rider.partner.is_some() && rider.partner_name.is_some() {
                    return Err(format!(
                        "Rider {name}: set either partner or partner_name, not both."
                    ));
                }
                let id = RiderId::new();
                let birthdate = rider.birthdate.midnight().assume_utc();
                let checked_in_at = rider.checked_in_at.map(utc);
                if rider.single.is_some() && rider.gender.is_some_and(|g| Some(g) != rider.single) {
                    return Err(format!("Rider {name}: single doesn't match the gender."));
                }
                let gender = rider.gender.or(rider.single).map(|g| match g {
                    Single::Female => Gender::Female,
                    Single::Male => Gender::Male,
                });
                repo.seed_rider(&SeedRider {
                    id,
                    club_id,
                    firstname: rider.firstname.clone(),
                    lastname: rider.lastname.clone(),
                    birthdate,
                    gender,
                    checked_in_at,
                    fee_waiver: rider.fee_waiver.clone(),
                })
                .await
                .map_err(|e| format!("Couldn't insert rider {name}: {e}"))?;
                riders.insert(name, id, rider);
            }
        }
        Ok(riders)
    }

    /// Registers the clubs' judges with the same rules as the API.
    async fn insert_judges(&self, repo: &dyn Db) -> Result<(), String> {
        // The seed acts as the organizer, like the other registrations.
        let organizer = db_core::clubs::Actor {
            user_id: UserId::new(),
            club_id: None,
            manage_registrations: true,
        };
        let units: HashMap<String, (crate::jury::JuryUnitKind, JuryUnitId)> = repo
            .jury_unit_rows()
            .await
            .map_err(|e| e.to_string())?
            .units
            .into_iter()
            .map(|unit| {
                (
                    crate::jury::unit_label(unit.kind, &unit.name),
                    (unit.kind, unit.id),
                )
            })
            .collect();
        for club in &self.clubs {
            if club.judges.is_empty() {
                continue;
            }
            let club_id = repo
                .find_club_by_name(&club.name)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Club {} doesn't exist.", club.name))?;
            for judge in &club.judges {
                let name = format!("{} {}", judge.firstname, judge.lastname);
                let registrations = judge
                    .registrations
                    .iter()
                    .map(|r| {
                        units
                            .get(&r.unit)
                            .map(|(kind, id)| crate::jury::JudgeRegistration {
                                unit_kind: *kind,
                                unit_id: *id,
                                task: r.task,
                            })
                            .ok_or_else(|| format!("Judge {name}: unknown jury unit {:?}", r.unit))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let fields = crate::jury::JudgeFields {
                    firstname: judge.firstname.clone(),
                    lastname: judge.lastname.clone(),
                    birthdate: judge.birthdate.midnight().assume_utc(),
                    is_trained: judge.trained,
                    max_deployments: judge.max_deployments,
                    note: judge.note.clone(),
                    registrations,
                };
                repo.add_judge(&organizer, club_id, fields)
                    .await
                    .map_err(|e| format!("Judge {name}: {e}"))?;
            }
        }
        Ok(())
    }

    /// Registers the singles, linked pairs and pair requests with the same
    /// rules as the API.
    async fn register_starts(&self, repo: &dyn Db, riders: &RiderIds<'_>) -> Result<(), String> {
        let mut partners: HashMap<&str, &str> = HashMap::new();
        for rider in self.clubs.iter().flat_map(|club| &club.riders) {
            let Some(partner) = rider.partner.as_deref() else {
                continue;
            };
            let name = rider.full_name();
            let partner_rider = riders.rider(partner, &name)?;
            if !partner_rider.pair {
                return Err(format!("Rider {name}: partner {partner} has pair = false."));
            }
            if partner_rider.partner_name.is_some()
                || partner_rider.partner.as_deref().is_some_and(|p| p != name)
            {
                return Err(format!(
                    "Rider {name}: partner {partner} already has another partner."
                ));
            }
            if partner == name {
                return Err(format!("Rider {name} can't be their own partner."));
            }
            let (name, partner) = (riders.name_ref(&name), riders.name_ref(partner));
            if let Some(other) = partners.get(name).filter(|p| **p != partner) {
                return Err(format!(
                    "Rider {name} is partner of both {other} and {partner}."
                ));
            }
            partners.insert(name, partner);
            partners.insert(partner, name);
        }

        // The seed acts as the organizer; each call is its own unit of work.
        let organizer = db_core::clubs::Actor {
            user_id: UserId::new(),
            club_id: None,
            manage_registrations: true,
        };
        for (&name, &partner) in &partners {
            // Each pair once.
            if name > partner {
                continue;
            }
            let partner_id = riders.id(partner, name)?;
            let id = riders.id(name, name)?;
            repo.link_pair(&organizer, id, partner_id)
                .await
                .map_err(|e| format!("Couldn't link partners {name} and {partner}: {e}"))?;
        }
        for (id, rider) in &riders.all {
            let name = rider.full_name();
            if rider.single.is_some() {
                repo.set_single(&organizer, *id, true)
                    .await
                    .map_err(|e| format!("Couldn't add the single of {name}: {e}"))?;
            }
            // Riders named as `partner` by their partner are linked above.
            if rider.pair && !partners.contains_key(name.as_str()) {
                repo.request_pair(&organizer, *id, rider.partner_name.as_deref())
                    .await
                    .map_err(|e| format!("Couldn't request a pair for {name}: {e}"))?;
            }
        }
        Ok(())
    }

    /// Creates the groups with their riders and sets their act type. Returns
    /// the act ids by group name.
    async fn insert_groups(
        &self,
        repo: &dyn Db,
        riders: &RiderIds<'_>,
    ) -> Result<HashMap<String, ActId>, String> {
        let db_error = |e: DbError| format!("Database error while seeding groups: {e}");
        let mut ids = HashMap::new();
        for group in &self.groups {
            let referenced_by = format!("Group {}", group.name);
            let group_name = groups::normalize_group_name(&group.name)
                .map_err(|e| format!("{referenced_by}: {e}"))?;
            let owner_club_id = repo
                .find_club_by_name(&group.owner)
                .await
                .map_err(db_error)?
                .ok_or_else(|| {
                    format!("{referenced_by}: owner {} is not a demo club.", group.owner)
                })?;
            if !group.act_type.is_group() {
                return Err(format!(
                    "{referenced_by}: act_type must be group_small or group_large."
                ));
            }

            let act_id = ActId::new();
            let mut seen = HashSet::new();
            let members = group
                .members
                .iter()
                .map(|name| (name, false))
                .chain(group.replacements.iter().map(|name| (name, true)));
            let mut participants = Vec::new();
            for (name, is_replacement) in members {
                let rider_id = riders.id(name, &referenced_by)?;
                if !seen.insert(rider_id) {
                    return Err(format!("{referenced_by}: {name} is listed twice."));
                }
                participants.push((rider_id, is_replacement));
            }
            repo.seed_group(&SeedGroup {
                id: act_id,
                owner_club_id,
                act_type: group.act_type,
                name: group_name,
                participants,
            })
            .await
            .map_err(|e| format!("Couldn't insert group {}: {e}", group.name))?;
            // The API only warns about such conflicts; demo groups must not
            // have any.
            if let Some(conflict) = repo.group_conflict(act_id).await.map_err(db_error)? {
                return Err(format!("{referenced_by}: {conflict}"));
            }
            ids.insert(group.name.clone(), act_id);
        }
        Ok(ids)
    }

    async fn update_acts(
        &self,
        repo: &dyn Db,
        riders: &RiderIds<'_>,
        group_ids: &HashMap<String, ActId>,
        data_path: &Path,
    ) -> Result<(), String> {
        let mut seen = HashSet::new();
        for (index, act) in self.acts.iter().enumerate() {
            let (act_id, label) = match (&act.group, &act.rider, act.act_type) {
                (Some(group), None, None) => {
                    let act_id = *group_ids
                        .get(group)
                        .ok_or_else(|| format!("[[acts]]: group {group} doesn't exist."))?;
                    (act_id, format!("group {group}"))
                }
                (None, Some(rider), Some(act_type)) => {
                    let rider_id = riders.id(rider, "[[acts]]")?;
                    let starts = repo
                        .rider_starts(rider_id)
                        .await
                        .map_err(|e| e.to_string())?;
                    let act_id = match (act_type, starts.pair) {
                        (ActType::Single, _) => starts.single,
                        (ActType::Pair, Pair::Linked { act_id, .. }) => Some(act_id),
                        _ => None,
                    }
                    .ok_or_else(|| {
                            format!(
                                "[[acts]]: {rider} has no {} act. Singles need `single`, pairs a linked `partner`.",
                                act_type.as_str()
                            )
                        })?;
                    (act_id, format!("{} act of {rider}", act_type.as_str()))
                }
                _ => {
                    return Err(
                        "[[acts]]: select an act either by rider and act_type, or by group."
                            .to_string(),
                    );
                }
            };
            if !seen.insert(act_id) {
                return Err(format!("[[acts]]: the {label} is described twice."));
            }

            let (song_file, song_file_name) = match (&act.song, &act.song_source) {
                (None, None) => (None, None),
                (song, source) => {
                    let (content, extension) = match source {
                        Some(source) => {
                            let path = self.base_dir.join(source);
                            let content = std::fs::read(&path).map_err(|e| {
                                format!("Couldn't read song {}: {e}", path.display())
                            })?;
                            let extension = path
                                .extension()
                                .map(|e| e.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "bin".to_string());
                            (content, extension)
                        }
                        None => (tone_wav(220.0 + 55.0 * (index % 8) as f64, 5), "wav".into()),
                    };
                    let file = format!("{act_id}.{extension}");
                    std::fs::create_dir_all(data_path).map_err(|e| {
                        format!("Couldn't create data path {}: {e}", data_path.display())
                    })?;
                    std::fs::write(data_path.join(&file), content)
                        .map_err(|e| format!("Couldn't write song {file}: {e}"))?;
                    let name = song.clone().unwrap_or_else(|| {
                        source
                            .as_ref()
                            .and_then(|s| s.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| file.clone())
                    });
                    (Some(file), Some(name))
                }
            };

            repo.seed_act_details(&SeedActDetails {
                id: act_id,
                name: act.name.clone().unwrap_or_default(),
                description: act.description.clone(),
                song_file,
                song_file_name,
                song_checked: act.song_checked,
                order: act.order,
                started_at: act.started_at.map(utc),
                ended_at: act.ended_at.map(utc),
                fee_waiver: act.fee_waiver.clone(),
            })
            .await
            .map_err(|e| format!("Couldn't update the {label}: {e}"))?;
        }
        Ok(())
    }
}

fn utc(datetime: OffsetDateTime) -> OffsetDateTime {
    datetime.to_offset(UtcOffset::UTC)
}

/// Riders by full name. Names used by more than one rider can't be
/// referenced.
#[derive(Default)]
struct RiderIds<'a> {
    by_name: HashMap<String, Option<(RiderId, &'a DemoRider)>>,
    /// Every rider, also those with an ambiguous name.
    all: Vec<(RiderId, &'a DemoRider)>,
}

impl<'a> RiderIds<'a> {
    fn insert(&mut self, name: String, id: RiderId, rider: &'a DemoRider) {
        self.all.push((id, rider));
        self.by_name
            .entry(name)
            .and_modify(|entry| *entry = None)
            .or_insert(Some((id, rider)));
    }

    fn len(&self) -> usize {
        self.by_name.len()
    }

    fn get(&self, name: &str, referenced_by: &str) -> Result<(RiderId, &'a DemoRider), String> {
        match self.by_name.get(name) {
            Some(Some(entry)) => Ok(*entry),
            Some(None) => Err(format!(
                "{referenced_by}: rider name {name} is ambiguous, it's used more than once."
            )),
            None => Err(format!("{referenced_by}: rider {name} doesn't exist.")),
        }
    }

    fn id(&self, name: &str, referenced_by: &str) -> Result<RiderId, String> {
        self.get(name, referenced_by).map(|(id, _)| id)
    }

    fn rider(&self, name: &str, referenced_by: &str) -> Result<&'a DemoRider, String> {
        self.get(name, referenced_by).map(|(_, rider)| rider)
    }

    /// The stored key for `name`, so it can be borrowed from `self`.
    fn name_ref(&self, name: &str) -> &str {
        self.by_name
            .get_key_value(name)
            .map(|(key, _)| key.as_str())
            .expect("name was resolved before")
    }
}

/// Downloads `url` to `target` with curl, so a broken download never ends up
/// at `target`.
fn download(url: &str, target: &Path) -> Result<(), String> {
    let partial = target.with_extension("part");
    let status = std::process::Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args(["--max-time", "300", "--output"])
        .arg(&partial)
        .arg(url)
        .status()
        .map_err(|e| format!("Couldn't run curl: {e}"))?;
    if !status.success() {
        std::fs::remove_file(&partial).ok();
        return Err(format!("curl failed ({status})"));
    }
    std::fs::rename(&partial, target)
        .map_err(|e| format!("Couldn't move the download to {}: {e}", target.display()))
}

/// A mono 16-bit PCM WAV file with a sine tone.
fn tone_wav(frequency: f64, seconds: u32) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 8000;
    let samples = SAMPLE_RATE * seconds;
    let data_len = samples * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = i as f64 / SAMPLE_RATE as f64;
        let sample = (t * frequency * std::f64::consts::TAU).sin() * 0.3 * i16::MAX as f64;
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_has_a_valid_wav_header() {
        let wav = tone_wav(440.0, 1);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 8000 * 2);
    }
}
