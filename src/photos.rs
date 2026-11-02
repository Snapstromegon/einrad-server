//! Photos for the info board: the newest image of a Nextcloud folder (or a
//! local one) after each act, random recent ones in breaks. The music
//! control previews the newest one.
//!
//! The source is a competition setting (`[competition.photos]`, ADR 0005).
//! Every [`POLL_EVERY`] the competition's photo settings are read, and while
//! there's a source, it's listed, the folder and its direct subfolders:
//! - a folder in a Nextcloud account over WebDAV,
//!   `https://cloud…/remote.php/dav/files/<user>/<path>`, with the user and an
//!   app password;
//! - a Nextcloud share link, `https://cloud…/s/<token>`, with the share's
//!   password if it has one;
//! - any other WebDAV folder, or a local folder (e.g. one the Nextcloud
//!   desktop client keeps in sync).
//!
//! From Nextcloud the images come scaled down by its preview API where
//! possible, else as they are. The newest few are kept in memory.

use std::{
    collections::{HashSet, VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use axum::body::Bytes;
use percent_encoding::percent_decode_str;
use reqwest::{
    Method,
    header::{CONTENT_TYPE, HeaderValue},
};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc2822};
use tracing::{info, warn};
use url::Url;
use utoipa::ToSchema;

use crate::{competition::Competition, reloadable_sqlite::ReloadableSqlite};

/// How often the source is listed.
pub const POLL_EVERY: Duration = Duration::from_secs(5);
/// How many of the newest photos breaks choose from.
const RECENT: usize = 30;
/// How many images are kept in memory.
const CACHED_IMAGES: usize = 8;
/// The longest side of the images fetched through Nextcloud's preview API.
const PREVIEW_SIZE: &str = "1920";

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
  <d:prop>
    <d:getlastmodified/>
    <d:getcontenttype/>
    <d:resourcetype/>
    <oc:fileid/>
  </d:prop>
</d:propfind>"#;

/// Where the photos come from, as the competition sets it
/// (`[competition.photos]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PhotosSettings {
    /// A WebDAV URL, a Nextcloud share link or a local folder.
    pub source: String,
    /// Nextcloud user for a WebDAV folder.
    #[serde(default)]
    pub user: Option<String>,
    /// Nextcloud app password for a WebDAV folder, or a share link's
    /// password.
    #[serde(default)]
    pub password: Option<String>,
}

impl PhotosSettings {
    /// Checks that the source is a folder path, a WebDAV URL or a share
    /// link with a token.
    pub fn check(&self) -> Result<(), String> {
        Source::parse(self).map(|_| ())
    }
}

// There's only one per server, so its size doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Local(PathBuf),
    WebDav {
        folder: Url,
        auth: Option<(String, String)>,
        preview: Preview,
    },
}

/// How Nextcloud scales an image down.
#[derive(Debug, Clone, PartialEq)]
enum Preview {
    /// A folder in an account: `index.php/core/preview?fileId=…`.
    Account {
        base: Url,
    },
    /// A share link: `index.php/apps/files_sharing/publicpreview/<token>`.
    Share {
        base: Url,
        token: String,
    },
    None,
}

impl Source {
    fn parse(settings: &PhotosSettings) -> Result<Self, String> {
        let source = settings.source.trim();
        if source.is_empty() {
            return Err("The photo source is empty".to_owned());
        }
        if !(source.starts_with("http://") || source.starts_with("https://")) {
            return Ok(Self::Local(PathBuf::from(source)));
        }
        let url = Url::parse(source).map_err(|e| format!("Invalid photo source {source}: {e}"))?;
        let path = url.path().to_owned();
        let auth = settings
            .user
            .clone()
            .map(|user| (user, settings.password.clone().unwrap_or_default()));
        // Nextcloud may live in a subfolder, e.g. https://example.org/cloud/.
        let base_at = |marker: &str| {
            path.find(marker).map(|at| {
                let mut base = url.clone();
                base.set_path(&path[..at + 1]);
                base.set_query(None);
                base
            })
        };
        if let Some(at) = path.find("/s/").filter(|_| !path.contains(".php/dav")) {
            let token = path[at + 3..].trim_end_matches('/').to_owned();
            let base = base_at("/index.php/s/")
                .or_else(|| base_at("/s/"))
                .expect("found");
            if token.is_empty() || token.contains('/') {
                return Err(format!("No share token in {source}"));
            }
            // Public shares over WebDAV: the token is the user.
            let folder = base.join("public.php/webdav/").map_err(|e| e.to_string())?;
            let password = settings.password.clone().unwrap_or_default();
            return Ok(Self::WebDav {
                folder,
                auth: Some((token.clone(), password)),
                preview: Preview::Share { base, token },
            });
        }
        let mut folder = url.clone();
        if !folder.path().ends_with('/') {
            folder.set_path(&format!("{path}/"));
        }
        let preview = match base_at("/remote.php/") {
            Some(base) => Preview::Account { base },
            None => Preview::None,
        };
        Ok(Self::WebDav {
            folder,
            auth,
            preview,
        })
    }
}

/// A photo in the source.
#[derive(Debug, Clone, PartialEq)]
struct Photo {
    /// Stable while the file doesn't change.
    id: String,
    /// The local path, or the WebDAV href.
    location: String,
    modified: OffsetDateTime,
    content_type: String,
    /// Nextcloud's file id, for its preview API.
    file_id: Option<String>,
}

impl Photo {
    fn new(
        location: String,
        modified: OffsetDateTime,
        content_type: String,
        file_id: Option<String>,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        (&location, modified.unix_timestamp_nanos()).hash(&mut hasher);
        Self {
            id: format!("{:016x}", hasher.finish()),
            location,
            modified,
            content_type,
            file_id,
        }
    }

    fn info(&self) -> PhotoInfo {
        PhotoInfo {
            id: self.id.clone(),
            modified: self.modified,
            url: format!("/api/photos/{}", self.id),
        }
    }
}

/// A photo as clients see it.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct PhotoInfo {
    pub id: String,
    /// When the file was last changed, e.g. uploaded.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub modified: OffsetDateTime,
    /// Where the image is, e.g. `/api/photos/<id>`.
    pub url: String,
}

/// An image, ready to send.
#[derive(Debug, Clone)]
pub struct Image {
    pub bytes: Bytes,
    pub content_type: String,
}

#[derive(Default)]
struct State {
    /// The settings `source` comes from.
    settings: Option<PhotosSettings>,
    source: Option<Source>,
    /// Counts the changes of the source, so a listing or an image of the
    /// old source isn't kept.
    generation: u64,
    /// Newest first.
    photos: Vec<Photo>,
    hidden: HashSet<String>,
    images: VecDeque<(String, Image)>,
    error: Option<String>,
}

/// The photos of the competition's source; see the module docs.
pub struct Photos {
    http: reqwest::Client,
    state: Mutex<State>,
}

impl Photos {
    /// No photos until [`Photos::configure`] sets a source.
    pub fn new() -> Result<Arc<Self>, String> {
        // reqwest uses rustls with ring; the first install wins.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            http,
            state: Mutex::new(State::default()),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn configured(&self) -> bool {
        self.lock().source.is_some()
    }

    /// Switches to `settings`. A change starts over: the old source's
    /// photos, hidden photos and images are dropped. An invalid source
    /// leaves no source, with the reason as the error. Returns whether the
    /// settings changed.
    pub fn configure(&self, settings: Option<&PhotosSettings>) -> bool {
        let mut state = self.lock();
        if state.settings.as_ref() == settings {
            return false;
        }
        let (source, error) = match settings.map(Source::parse) {
            None => (None, None),
            Some(Ok(source)) => (Some(source), None),
            Some(Err(e)) => (None, Some(e)),
        };
        match (&settings, &error) {
            (None, _) => info!("No photo source; the photos are off"),
            (Some(_), Some(e)) => warn!("{e}; the photos are off"),
            (Some(settings), None) => info!("The photo source is {}", settings.source),
        }
        *state = State {
            settings: settings.cloned(),
            source,
            generation: state.generation + 1,
            error,
            ..State::default()
        };
        true
    }

    /// Follows the competition's photo settings in the background: every
    /// [`POLL_EVERY`] they're read from `db` and the source is listed (see
    /// [`Photos::sync`]).
    pub fn start(self: &Arc<Self>, db: ReloadableSqlite) {
        let photos = self.clone();
        tokio::spawn(async move {
            loop {
                let pool = db.get().await.clone();
                if let Err(e) = photos.sync(&pool).await {
                    warn!("Couldn't list the photos: {e}");
                }
                tokio::time::sleep(POLL_EVERY).await;
            }
        });
    }

    /// Takes the current competition's photo settings (see
    /// [`Photos::configure`]) and lists the source once.
    pub async fn sync(&self, db: &sqlx::SqlitePool) -> Result<(), String> {
        let competition = Competition::current(db)
            .await
            .map_err(|e| format!("Couldn't read the photo settings: {e}"))?;
        self.configure(competition.and_then(|c| c.photos).as_ref());
        self.refresh().await
    }

    /// Lists the source once.
    pub async fn refresh(&self) -> Result<(), String> {
        let (source, generation) = {
            let state = self.lock();
            match &state.source {
                Some(source) => (source.clone(), state.generation),
                None => return Ok(()),
            }
        };
        let listed = match &source {
            Source::Local(dir) => {
                let dir = dir.clone();
                tokio::task::spawn_blocking(move || list_local(&dir))
                    .await
                    .map_err(|e| e.to_string())?
            }
            Source::WebDav { folder, auth, .. } => self.list_webdav(folder, auth.as_ref()).await,
        };
        let mut state = self.lock();
        if state.generation != generation {
            return Ok(());
        }
        match listed {
            Ok(mut photos) => {
                photos.sort_by_key(|photo| std::cmp::Reverse(photo.modified));
                let newest = |photos: &[Photo]| photos.first().map(|p| p.id.clone());
                if newest(&photos) != newest(&state.photos) {
                    info!(
                        "{} photos, the newest from {:?}",
                        photos.len(),
                        photos.first().map(|p| p.modified)
                    );
                }
                state.photos = photos;
                state.error = None;
                Ok(())
            }
            Err(e) => {
                state.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// The newest photo that isn't hidden.
    pub fn newest(&self) -> Option<PhotoInfo> {
        let state = self.lock();
        state
            .photos
            .iter()
            .find(|photo| !state.hidden.contains(&photo.id))
            .map(Photo::info)
    }

    /// The newest photos that aren't hidden, newest first.
    pub fn recent(&self) -> Vec<PhotoInfo> {
        let state = self.lock();
        state
            .photos
            .iter()
            .filter(|photo| !state.hidden.contains(&photo.id))
            .take(RECENT)
            .map(Photo::info)
            .collect()
    }

    /// Why the last listing failed, if it did.
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Hides a photo from the info board, or shows it again. False for an
    /// unknown photo.
    pub fn set_hidden(&self, id: &str, hidden: bool) -> bool {
        let mut state = self.lock();
        if !state.photos.iter().any(|photo| photo.id == id) {
            return false;
        }
        if hidden {
            state.hidden.insert(id.to_owned());
        } else {
            state.hidden.remove(id);
        }
        true
    }

    /// A photo's image, unless it's unknown or hidden.
    pub async fn image(&self, id: &str) -> Result<Option<Image>, String> {
        let (photo, source, generation) = {
            let state = self.lock();
            if state.hidden.contains(id) {
                return Ok(None);
            }
            if let Some((_, image)) = state.images.iter().find(|(cached, _)| cached == id) {
                return Ok(Some(image.clone()));
            }
            match (
                state.photos.iter().find(|photo| photo.id == id),
                &state.source,
            ) {
                (Some(photo), Some(source)) => (photo.clone(), source.clone(), state.generation),
                _ => return Ok(None),
            }
        };
        let image = self.fetch(&source, &photo).await?;
        let mut state = self.lock();
        if state.generation != generation {
            return Ok(Some(image));
        }
        state.images.push_front((photo.id, image.clone()));
        state.images.truncate(CACHED_IMAGES);
        Ok(Some(image))
    }

    async fn fetch(&self, source: &Source, photo: &Photo) -> Result<Image, String> {
        let Source::WebDav {
            folder,
            auth,
            preview,
        } = source
        else {
            let bytes = tokio::fs::read(&photo.location)
                .await
                .map_err(|e| format!("{}: {e}", photo.location))?;
            return Ok(Image {
                bytes: bytes.into(),
                content_type: photo.content_type.clone(),
            });
        };
        if let Some((url, auth)) = preview_url(preview, folder, auth.as_ref(), photo) {
            match self.get(url, auth).await {
                Ok(image) if image.content_type.starts_with("image/") => return Ok(image),
                Ok(_) => warn!("Nextcloud's preview of {} is no image", photo.location),
                Err(e) => warn!("No preview of {}: {e}", photo.location),
            }
        }
        let url = folder.join(&photo.location).map_err(|e| e.to_string())?;
        let mut image = self.get(url, auth.as_ref()).await?;
        if !image.content_type.starts_with("image/") {
            image.content_type = photo.content_type.clone();
        }
        Ok(image)
    }

    async fn get(&self, url: Url, auth: Option<&(String, String)>) -> Result<Image, String> {
        let mut request = self.http.get(url.clone());
        if let Some((user, password)) = auth {
            request = request.basic_auth(user, Some(password));
        }
        let response = request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("{url}: {e}"))?;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = response.bytes().await.map_err(|e| format!("{url}: {e}"))?;
        Ok(Image {
            bytes,
            content_type,
        })
    }

    async fn list_webdav(
        &self,
        folder: &Url,
        auth: Option<&(String, String)>,
    ) -> Result<Vec<Photo>, String> {
        let mut photos = vec![];
        let mut subfolders = vec![];
        for entry in self.propfind(folder, auth).await? {
            if same_path(&entry.href, folder.path()) {
                continue;
            }
            if entry.collection {
                subfolders.push(entry.href);
            } else if let Some(photo) = entry.photo() {
                photos.push(photo);
            }
        }
        for href in subfolders {
            let url = folder.join(&href).map_err(|e| e.to_string())?;
            match self.propfind(&url, auth).await {
                Ok(entries) => photos.extend(
                    entries
                        .into_iter()
                        .filter(|entry| !entry.collection)
                        .filter_map(|entry| entry.photo()),
                ),
                Err(e) => warn!("Couldn't list the photos in {href}: {e}"),
            }
        }
        Ok(photos)
    }

    async fn propfind(
        &self,
        url: &Url,
        auth: Option<&(String, String)>,
    ) -> Result<Vec<DavEntry>, String> {
        let mut request = self
            .http
            .request(
                Method::from_bytes(b"PROPFIND").expect("valid method"),
                url.clone(),
            )
            .header("Depth", "1")
            .header(
                CONTENT_TYPE,
                HeaderValue::from_static("application/xml; charset=utf-8"),
            )
            .body(PROPFIND_BODY);
        if let Some((user, password)) = auth {
            request = request.basic_auth(user, Some(password));
        }
        let response = request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("{url}: {e}"))?;
        let xml = response.text().await.map_err(|e| format!("{url}: {e}"))?;
        parse_multistatus(&xml)
    }
}

/// The preview URL of a photo from Nextcloud, with the login it needs.
fn preview_url<'a>(
    preview: &Preview,
    folder: &Url,
    auth: Option<&'a (String, String)>,
    photo: &Photo,
) -> Option<(Url, Option<&'a (String, String)>)> {
    match preview {
        Preview::Account { base } => {
            let mut url = base.join("index.php/core/preview").ok()?;
            url.query_pairs_mut()
                .append_pair("fileId", photo.file_id.as_deref()?)
                .append_pair("x", PREVIEW_SIZE)
                .append_pair("y", PREVIEW_SIZE)
                .append_pair("a", "1");
            Some((url, auth))
        }
        Preview::Share { base, token } => {
            // The path inside the share, e.g. "/Freitag/IMG_1.jpg".
            let root = folder.path().trim_end_matches('/');
            let inside = photo.location.strip_prefix(root)?;
            let inside = percent_decode_str(inside).decode_utf8().ok()?;
            let mut url = base
                .join(&format!(
                    "index.php/apps/files_sharing/publicpreview/{token}"
                ))
                .ok()?;
            url.query_pairs_mut()
                .append_pair("file", &inside)
                .append_pair("x", PREVIEW_SIZE)
                .append_pair("y", PREVIEW_SIZE)
                .append_pair("a", "1");
            Some((url, None))
        }
        Preview::None => None,
    }
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The image types browsers show; others (e.g. HEIC, raw) are left out.
fn image_type(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// The images in a local folder and its direct subfolders.
fn list_local(dir: &Path) -> Result<Vec<Photo>, String> {
    let mut photos = vec![];
    let mut dirs = vec![(dir.to_path_buf(), true)];
    while let Some((dir, top)) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if top => return Err(format!("{}: {e}", dir.display())),
            Err(e) => {
                warn!("Couldn't list the photos in {}: {e}", dir.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if top {
                    dirs.push((path, false));
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let (Some(content_type), Ok(modified)) = (image_type(&name), metadata.modified())
            else {
                continue;
            };
            photos.push(Photo::new(
                path.to_string_lossy().into_owned(),
                modified.into(),
                content_type.to_owned(),
                None,
            ));
        }
    }
    Ok(photos)
}

/// An entry of a WebDAV `PROPFIND` answer.
#[derive(Debug, PartialEq)]
struct DavEntry {
    href: String,
    modified: Option<OffsetDateTime>,
    content_type: Option<String>,
    collection: bool,
    file_id: Option<String>,
}

impl DavEntry {
    fn photo(self) -> Option<Photo> {
        let decoded = percent_decode_str(&self.href)
            .decode_utf8_lossy()
            .into_owned();
        let content_type = self
            .content_type
            .filter(|t| {
                matches!(
                    t.as_str(),
                    "image/jpeg" | "image/png" | "image/webp" | "image/gif"
                )
            })
            .or_else(|| image_type(&decoded).map(str::to_owned))?;
        Some(Photo::new(
            self.href,
            self.modified?,
            content_type,
            self.file_id,
        ))
    }
}

fn parse_multistatus(xml: &str) -> Result<Vec<DavEntry>, String> {
    const DAV: &str = "DAV:";
    const OWNCLOUD: &str = "http://owncloud.org/ns";
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Invalid WebDAV answer: {e}"))?;
    let text = |node: roxmltree::Node, name: (&str, &str)| {
        node.descendants()
            .find(|n| n.has_tag_name(name))
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name((DAV, "response")))
        .filter_map(|response| {
            Some(DavEntry {
                href: text(response, (DAV, "href"))?,
                modified: text(response, (DAV, "getlastmodified")).and_then(|date| {
                    OffsetDateTime::parse(&date.replace(" GMT", " +0000"), &Rfc2822).ok()
                }),
                content_type: text(response, (DAV, "getcontenttype"))
                    .map(|t| t.split(';').next().unwrap_or_default().trim().to_owned()),
                collection: response
                    .descendants()
                    .any(|n| n.has_tag_name((DAV, "collection"))),
                file_id: text(response, (OWNCLOUD, "fileid")),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(source: &str, user: Option<&str>, password: Option<&str>) -> PhotosSettings {
        PhotosSettings {
            source: source.into(),
            user: user.map(Into::into),
            password: password.map(Into::into),
        }
    }

    fn url(url: &str) -> Url {
        Url::parse(url).unwrap()
    }

    #[test]
    fn a_new_source_starts_over() {
        let photos = Photos::new().unwrap();
        assert!(!photos.configured());
        assert!(photos.configure(Some(&settings("/srv/fotos", None, None))));
        assert!(!photos.configure(Some(&settings("/srv/fotos", None, None))));
        assert!(photos.configured());
        photos.lock().photos.push(Photo::new(
            "/srv/fotos/a.jpg".into(),
            OffsetDateTime::UNIX_EPOCH,
            "image/jpeg".into(),
            None,
        ));
        assert!(photos.newest().is_some());

        assert!(photos.configure(Some(&settings("/srv/andere", None, None))));
        assert_eq!(photos.newest(), None);

        assert!(photos.configure(Some(&settings(" ", None, None))));
        assert!(!photos.configured());
        assert!(photos.error().unwrap().contains("empty"));
        assert!(photos.configure(None));
        assert_eq!(photos.error(), None);
    }

    #[test]
    fn sources_are_recognized() {
        assert_eq!(
            Source::parse(&settings("/srv/fotos", None, None)),
            Ok(Source::Local("/srv/fotos".into()))
        );
        assert_eq!(
            Source::parse(&settings(
                "https://cloud.example.org/remote.php/dav/files/anna/Fotos",
                Some("anna"),
                Some("app-password")
            )),
            Ok(Source::WebDav {
                folder: url("https://cloud.example.org/remote.php/dav/files/anna/Fotos/"),
                auth: Some(("anna".into(), "app-password".into())),
                preview: Preview::Account {
                    base: url("https://cloud.example.org/")
                },
            })
        );
        // A share link, also of a Nextcloud in a subfolder.
        assert_eq!(
            Source::parse(&settings(
                "https://example.org/cloud/index.php/s/AbC123",
                None,
                None
            )),
            Ok(Source::WebDav {
                folder: url("https://example.org/cloud/public.php/webdav/"),
                auth: Some(("AbC123".into(), String::new())),
                preview: Preview::Share {
                    base: url("https://example.org/cloud/"),
                    token: "AbC123".into()
                },
            })
        );
        assert_eq!(
            Source::parse(&settings(
                "https://cloud.example.org/s/AbC123/",
                None,
                Some("geheim")
            )),
            Ok(Source::WebDav {
                folder: url("https://cloud.example.org/public.php/webdav/"),
                auth: Some(("AbC123".into(), "geheim".into())),
                preview: Preview::Share {
                    base: url("https://cloud.example.org/"),
                    token: "AbC123".into()
                },
            })
        );
    }

    #[test]
    fn a_propfind_answer_is_read() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
  <d:response>
    <d:href>/remote.php/dav/files/anna/Fotos/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/anna/Fotos/Kür%201.JPG</d:href>
    <d:propstat>
      <d:prop>
        <d:getlastmodified>Tue, 13 Oct 2026 10:00:00 GMT</d:getlastmodified>
        <d:getcontenttype>image/jpeg</d:getcontenttype>
        <d:resourcetype/>
        <oc:fileid>42</oc:fileid>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_multistatus(xml).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].collection);
        let photo = entries.into_iter().nth(1).unwrap().photo().unwrap();
        assert_eq!(
            photo.location,
            "/remote.php/dav/files/anna/Fotos/Kür%201.JPG"
        );
        assert_eq!(photo.content_type, "image/jpeg");
        assert_eq!(photo.file_id.as_deref(), Some("42"));
        assert_eq!(photo.modified.unix_timestamp(), 1_791_885_600);
    }

    #[test]
    fn previews_come_from_nextcloud() {
        let photo = Photo::new(
            "/cloud/public.php/webdav/Freitag/Kür%201.jpg".into(),
            OffsetDateTime::UNIX_EPOCH,
            "image/jpeg".into(),
            Some("42".into()),
        );
        let share = Preview::Share {
            base: url("https://example.org/cloud/"),
            token: "AbC123".into(),
        };
        let folder = url("https://example.org/cloud/public.php/webdav/");
        let (preview, auth) = preview_url(&share, &folder, None, &photo).unwrap();
        assert_eq!(
            preview.as_str(),
            "https://example.org/cloud/index.php/apps/files_sharing/publicpreview/AbC123?file=%2FFreitag%2FK%C3%BCr+1.jpg&x=1920&y=1920&a=1"
        );
        assert_eq!(auth, None);

        let login = ("anna".to_owned(), "app-password".to_owned());
        let account = Preview::Account {
            base: url("https://cloud.example.org/"),
        };
        let (preview, auth) = preview_url(&account, &folder, Some(&login), &photo).unwrap();
        assert_eq!(
            preview.as_str(),
            "https://cloud.example.org/index.php/core/preview?fileId=42&x=1920&y=1920&a=1"
        );
        assert_eq!(auth, Some(&login));
    }
}
