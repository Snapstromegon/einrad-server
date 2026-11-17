//! Seeding and import of the competition (ADR 0005), the export, and the
//! demo data: the plan the backend writes and the [`SeedRepo`] and
//! [`ExportRepo`] traits (#238).
//!
//! The root crate turns a competition file into a [`CompetitionPlan`]: all
//! checks and derived values are pure and done up front. The backend then
//! writes the plan in one transaction, so an import is atomic.

use std::path::PathBuf;

use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    act_type::ActType,
    branding::Branding,
    category_ages::ResolvedCategory,
    colors::HexColor,
    competition::Competition,
    finals::SlotGroup,
    gender::Gender,
    heats::Discipline,
    ids::{ActId, ClubId, RiderId, UserId},
    jury::{RemapRegistration, UnitSettings},
    media::{Format, Image, Logo, StoredLogo},
    roles::Role,
    timeplan::EntryKind,
};

/// A final from the config with its durations in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFinal {
    /// The readable name.
    pub name: String,
    /// The short code, which the timeplan refers to.
    pub code: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    pub slots: Vec<SlotGroup>,
    pub jury: UnitSettings,
}

/// A heat from the config, checked against HEAT-1.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHeat {
    /// The readable name.
    pub name: String,
    /// The short code, which slots and categories refer to.
    pub code: String,
    pub description: Option<String>,
    pub categories: Vec<String>,
    /// The discipline its categories share; `None` without categories.
    pub discipline: Option<Discipline>,
    /// The file's, or the default for its categories.
    pub threshold: i64,
    pub skip: bool,
    pub jury: UnitSettings,
}

/// How the file sets the logo or the break image.
#[derive(Debug, Clone, PartialEq)]
pub enum ImagePlan {
    /// The file doesn't say: the database's image stays.
    Keep,
    /// The file resets it.
    Reset,
    /// An inline SVG.
    Svg(Logo),
    /// An image to download.
    Url(String),
}

/// Where a sponsor's image comes from, if the file gives one.
#[derive(Debug, Clone, PartialEq)]
pub enum SponsorImage {
    Svg(Logo),
    Url(String),
}

/// A sponsor of the file, with the name and link already cleaned.
#[derive(Debug, Clone, PartialEq)]
pub struct SponsorPlan {
    pub name: String,
    pub link: Option<String>,
    pub image: Option<SponsorImage>,
}

/// A timeplan entry of the file, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedEntry {
    pub kind: EntryKind,
    pub earliest_start: Option<OffsetDateTime>,
    pub duration_seconds: Option<i64>,
    pub einfahrzeit_seconds: Option<i64>,
    pub label: Option<String>,
    /// The category's name.
    pub category: Option<String>,
    /// The final's code.
    pub final_code: Option<String>,
    pub show_photos: bool,
    pub internal: bool,
}

/// Everything a competition file writes, checked and resolved. The backend
/// writes it as it is.
#[derive(Debug, Clone)]
pub struct CompetitionPlan {
    pub competition: Competition,
    pub primary_color: Branding<HexColor>,
    pub secondary_color: Branding<HexColor>,
    pub logo: ImagePlan,
    pub break_image: ImagePlan,
    /// `None`: the file says nothing, the sponsors stay.
    pub sponsors: Option<Vec<SponsorPlan>>,
    pub categories: Vec<ResolvedCategory>,
    pub heats: Vec<ResolvedHeat>,
    pub finals: Vec<ResolvedFinal>,
    pub timeplan: Vec<PlannedEntry>,
}

/// Builds the plan for a competition id: the id of a replace is the
/// database's, not the file's. Fails with the message for an invalid file.
pub type PlanBuilder<'a> = &'a (dyn Fn(&str) -> Result<CompetitionPlan, String> + Send + Sync);

/// What applying the competition file did.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    /// No competition file was given.
    NoFile,
    /// The database had no competition; the file created it.
    Created,
    /// The database already has the file's competition.
    Unchanged,
    /// The database has a competition that differs from the file; the file
    /// is ignored.
    Ignored,
    /// The file replaced the database's competition (`replace`).
    Replaced(Dropped),
}

/// A part of the competition that a file can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Settings,
    Images,
    Colors,
    Categories,
    Heats,
    Finals,
    Timeplan,
}

impl Part {
    /// For warnings, e.g. "the categories".
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Part::Settings => "the competition settings",
            Part::Images => "the logo, break image and sponsors",
            Part::Colors => "the colors",
            Part::Categories => "the categories",
            Part::Heats => "the heats",
            Part::Finals => "the finals",
            Part::Timeplan => "the timeplan",
        }
    }
}

/// The parts for a warning, e.g. "the heats, the finals".
#[must_use]
pub fn describe(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|part| part.describe())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a replace drops along with the old competition's planning.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct Dropped {
    pub timeplan_slots: i64,
    /// Acts started or ended, whose progress is reset.
    pub act_progress: i64,
    /// Acts placed in finals slots.
    pub finals_placements: i64,
    /// The jury plan's assignments.
    pub jury_assignments: i64,
    pub jury_plan_published: bool,
    /// Judge registrations whose jury unit (heat or final) has no namesake in
    /// the file.
    pub judge_registrations: Vec<RemapRegistration>,
    /// Where they were written, to fill in `jury_unit` and apply again.
    #[serde(skip)]
    pub remap_file: Option<PathBuf>,
}

impl Dropped {
    /// The dropped items for a warning, e.g. "12 timeplan slots, the
    /// published jury plan".
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        let counted = |count: i64, one: &str, many: &str| {
            (count > 0).then(|| format!("{count} {}", if count == 1 { one } else { many }))
        };
        [
            counted(self.timeplan_slots, "timeplan slot", "timeplan slots"),
            (self.act_progress > 0).then(|| {
                format!(
                    "the progress of {} act{}",
                    self.act_progress,
                    if self.act_progress == 1 { "" } else { "s" }
                )
            }),
            counted(
                self.finals_placements,
                "finals placement",
                "finals placements",
            ),
            counted(
                self.jury_assignments,
                "jury plan assignment",
                "jury plan assignments",
            ),
            self.jury_plan_published
                .then(|| "the published jury plan".to_string()),
            counted(
                self.judge_registrations.len() as i64,
                "judge registration without a matching heat or final",
                "judge registrations without a matching heat or final",
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// The result of applying a competition file in the backend.
#[derive(Debug)]
pub struct ApplyOutcome {
    pub applied: Applied,
    pub warnings: Vec<String>,
    /// The judge registrations that lost their jury unit, for the remap
    /// file; written after the commit.
    pub dropped_registrations: Vec<RemapRegistration>,
}

/// What a preview in the backend found; the file's id and name are added by
/// the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewOutcome {
    pub current_id: Option<String>,
    pub changes: Vec<Part>,
    pub dropped: Dropped,
}

/// Why seeding failed.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The file (or the request) is invalid or refused; the message is
    /// meant for the user.
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// A user of the demo data, with the password already hashed.
#[derive(Debug, Clone)]
pub struct SeedUser {
    pub id: UserId,
    pub email: String,
    pub email_verified: bool,
    pub name: String,
    pub password_hash: String,
    pub roles: Vec<Role>,
    pub verification_token: Option<Uuid>,
    pub password_reset_token: Option<Uuid>,
    /// When the tokens were created.
    pub now: OffsetDateTime,
}

/// A club of the demo data. A payment is booked as one transfer.
#[derive(Debug, Clone)]
pub struct SeedClub {
    pub id: ClubId,
    pub name: String,
    pub owner: UserId,
    pub fee_waiver: Option<String>,
    pub payment: Option<f64>,
    pub payment_date: Date,
}

/// A rider of the demo data.
#[derive(Debug, Clone)]
pub struct SeedRider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
    pub birthdate: OffsetDateTime,
    pub gender: Option<Gender>,
    pub checked_in_at: Option<OffsetDateTime>,
    pub fee_waiver: Option<String>,
}

/// A group act of the demo data.
#[derive(Debug, Clone)]
pub struct SeedGroup {
    pub id: ActId,
    pub owner_club_id: ClubId,
    pub act_type: ActType,
    pub name: String,
    /// The riders, with whether they are replacements.
    pub participants: Vec<(RiderId, bool)>,
}

/// The details the demo data sets on an act.
#[derive(Debug, Clone)]
pub struct SeedActDetails {
    pub id: ActId,
    pub name: String,
    pub description: Option<String>,
    pub song_file: Option<String>,
    pub song_file_name: Option<String>,
    pub song_checked: bool,
    pub order: Option<i64>,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
    pub fee_waiver: Option<String>,
}

/// Writing the competition file and the demo data.
#[async_trait::async_trait]
pub trait SeedRepo {
    /// Applies a competition file in one transaction (ADR 0005): creates the
    /// competition if the database has none; otherwise it is ignored with a
    /// warning if it differs, unless `replace`. `file_id` is the id in the
    /// file. An invalid file changes nothing.
    async fn apply_competition_file(
        &self,
        file_id: &str,
        replace: bool,
        plan: PlanBuilder<'_>,
    ) -> Result<ApplyOutcome, SeedError>;

    /// Applies the file as a replace does, in a transaction that is rolled
    /// back: nothing changes.
    async fn preview_competition_file(
        &self,
        file_id: &str,
        plan: PlanBuilder<'_>,
    ) -> Result<PreviewOutcome, SeedError>;

    /// Creates the competition of the plan in setup mode; refused with a
    /// German message while a competition exists (ADR 0001).
    async fn create_competition(
        &self,
        file_id: &str,
        plan: PlanBuilder<'_>,
    ) -> Result<(), SeedError>;

    /// Sets when the timeplan entries started and ended, in the order of
    /// their positions.
    async fn seed_timeplan_progress(
        &self,
        progress: &[(Option<OffsetDateTime>, Option<OffsetDateTime>)],
    ) -> Result<(), DbError>;

    /// A user with roles and pending tokens.
    async fn seed_user(&self, user: &SeedUser) -> Result<(), DbError>;

    /// A club, its payment and its owner's membership.
    async fn seed_club(&self, club: &SeedClub) -> Result<(), DbError>;

    async fn seed_rider(&self, rider: &SeedRider) -> Result<(), DbError>;

    /// The club with this name.
    async fn find_club_by_name(&self, name: &str) -> Result<Option<ClubId>, DbError>;

    /// A group act with its riders.
    async fn seed_group(&self, group: &SeedGroup) -> Result<(), DbError>;

    /// Sets the details of an act.
    async fn seed_act_details(&self, details: &SeedActDetails) -> Result<(), DbError>;
}

/// An image of the export, with its content when it is stored inline.
#[derive(Debug, Clone)]
pub struct DumpImage {
    pub image: Image,
    /// Loaded for images without a source URL that have data.
    pub stored: Option<StoredLogo>,
}

impl DumpImage {
    /// The format name for the note on a raster image the file leaves out.
    #[must_use]
    pub fn format_name(&self) -> &'static str {
        self.image.format().map_or("raster", Format::name)
    }
}

#[derive(Debug, Clone)]
pub struct DumpCategory {
    pub name: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    pub min_age: Option<f64>,
    pub under_age: Option<f64>,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct DumpHeat {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub threshold: i64,
    pub skip: bool,
    pub jury: UnitSettings,
    /// The names of its categories, in category order.
    pub categories: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DumpFinal {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    pub jury: UnitSettings,
    /// The code of each slot's heat, in slot order.
    pub slot_heats: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DumpEntry {
    pub kind: EntryKind,
    pub category: Option<String>,
    pub final_code: Option<String>,
    pub label: Option<String>,
    pub duration_seconds: Option<i64>,
    pub einfahrzeit_seconds: Option<i64>,
    pub show_photos: bool,
    pub internal: bool,
    pub earliest_start: Option<OffsetDateTime>,
}

/// The competition and its structure as the database has them, read in one
/// transaction; the root turns it into a competition file.
#[derive(Debug, Clone)]
pub struct CompetitionDump {
    pub competition: Competition,
    pub images: Vec<DumpImage>,
    pub categories: Vec<DumpCategory>,
    pub heats: Vec<DumpHeat>,
    pub finals: Vec<DumpFinal>,
    pub timeplan: Vec<DumpEntry>,
}

/// Reading the competition for the export.
#[async_trait::async_trait]
pub trait ExportRepo {
    /// The current competition and its structure; `None` without one.
    async fn dump_competition(&self) -> Result<Option<CompetitionDump>, DbError>;
}
