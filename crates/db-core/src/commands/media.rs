//! Media commands: logos, break images and sponsors.
//!
//! The request types are DB-facing. Decoding and checking an image (upload,
//! inline SVG, preset) and downloading a URL stay with the caller: a command
//! only stores the finished [`Logo`] or the checked URL. A refusal rolls
//! everything back; `NotFound` means there is no competition or no such
//! sponsor.

use crate::{
    actor::Actor,
    error::CommandError,
    media::{Logo, Slot},
};

/// Stores the competition's logo, replacing an earlier logo or URL source.
#[derive(Debug, Clone)]
pub struct SetCompetitionLogo {
    pub logo: Logo,
}

/// Stores the info board's break image, replacing an earlier one or URL source.
#[derive(Debug, Clone)]
pub struct SetBreakImage {
    pub logo: Logo,
}

/// Stores a sponsor's logo, replacing an earlier image or URL source.
#[derive(Debug, Clone)]
pub struct UploadSponsorImage {
    pub sponsor_id: i64,
    pub logo: Logo,
}

/// Removes a sponsor and its image.
#[derive(Debug, Clone)]
pub struct RemoveSponsor {
    pub id: i64,
}

/// Where a new image comes from: an already checked image, or a checked URL
/// the caller downloads right after (the command downloads nothing).
#[derive(Debug, Clone)]
pub enum ImageInput {
    /// An inline SVG, an upload or a preset.
    Data(Logo),
    /// An `http(s)` URL; the old bytes stay until the first good download.
    Url(String),
}

/// Adds a sponsor at the end of the list, with its image if given, in one
/// transaction. Returns the sponsor's id.
#[derive(Debug, Clone)]
pub struct AddSponsor {
    /// Trimmed and checked by the handler.
    pub name: String,
    pub link: Option<String>,
    pub image: Option<ImageInput>,
}

/// Changes a sponsor's name and link.
#[derive(Debug, Clone)]
pub struct UpdateSponsor {
    pub id: i64,
    pub name: String,
    pub link: Option<String>,
}

/// Sets the order of the sponsors. `ids` must name every sponsor exactly once,
/// else the command is `Rejected` and nothing changes.
#[derive(Debug, Clone)]
pub struct ReorderSponsors {
    pub ids: Vec<i64>,
}

/// Sets the source of the image in `slot`; the same URL changes nothing.
#[derive(Debug, Clone)]
pub struct SetImageSource {
    pub slot: Slot,
    pub source: ImageInput,
}

/// The DB part of `refresh_images`: what a download of `url` for image `id`
/// gave. The result is dropped if the image is gone or has another source by
/// now (the admin changed it while the download ran).
#[derive(Debug, Clone)]
pub struct StoreFetchedImage {
    pub id: i64,
    pub url: String,
    /// The checked image, or why the download failed.
    pub result: Result<Logo, String>,
}

/// The write side of the media area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait MediaCommands: Send + Sync {
    async fn set_competition_logo(
        &self,
        actor: &Actor,
        cmd: SetCompetitionLogo,
    ) -> Result<(), CommandError>;

    /// Removes the logo, so the one built into the binary applies again.
    async fn remove_competition_logo(&self, actor: &Actor) -> Result<(), CommandError>;

    async fn set_break_image(&self, actor: &Actor, cmd: SetBreakImage) -> Result<(), CommandError>;

    /// Removes the break image, so the logo applies again.
    async fn remove_break_image(&self, actor: &Actor) -> Result<(), CommandError>;

    async fn upload_sponsor_image(
        &self,
        actor: &Actor,
        cmd: UploadSponsorImage,
    ) -> Result<(), CommandError>;

    /// Returns the new sponsor's id.
    async fn add_sponsor(&self, actor: &Actor, cmd: AddSponsor) -> Result<i64, CommandError>;

    async fn update_sponsor(&self, actor: &Actor, cmd: UpdateSponsor) -> Result<(), CommandError>;

    async fn remove_sponsor(&self, actor: &Actor, cmd: RemoveSponsor) -> Result<(), CommandError>;

    async fn reorder_sponsors(
        &self,
        actor: &Actor,
        cmd: ReorderSponsors,
    ) -> Result<(), CommandError>;

    async fn set_image_source(
        &self,
        actor: &Actor,
        cmd: SetImageSource,
    ) -> Result<(), CommandError>;

    /// The DB part of `refresh_images` (the downloads stay in the server's
    /// `ImageFetcher`). A background job, so no actor. Returns `false` if the
    /// result was dropped.
    async fn store_fetched_image(&self, cmd: StoreFetchedImage) -> Result<bool, CommandError>;
}
