//! Downloads the competition's URL images (#291, ADR 0010): at startup, by
//! the "Bilder aktualisieren" button and when an admin sets a URL. A download
//! gets the same checks as an upload (`crate::logo`), and the server serves
//! only the local copy, never the URL.
//!
//! The URL comes from an admin or the competition file, so the server fetches
//! whatever they name, including addresses in its own network. ADR 0010 notes
//! that. Redirects are limited, and the response is cut off at the size limit
//! while it streams.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use reqwest::{StatusCode, header::USER_AGENT, redirect};
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::{info, warn};

use crate::{
    images::{self, Image, Slot},
    logo::{self, Logo},
    reloadable_sqlite::ReloadableSqlite,
};

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a whole download may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most redirects followed.
const MAX_REDIRECTS: usize = 3;
/// How many images are downloaded at once.
const PARALLEL: usize = 4;

/// Why a download failed. The messages are shown in the competition
/// management.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FetchError {
    #[error("Zeitüberschreitung beim Herunterladen.")]
    Timeout,
    #[error("Der Server ist nicht erreichbar.")]
    Unreachable,
    #[error("Der Server antwortet mit dem Status {0}.")]
    Status(u16),
    #[error("Die Antwort ist größer als 1 MiB.")]
    TooLarge,
    #[error("Das Herunterladen ist fehlgeschlagen.")]
    Failed,
    #[error("{0}")]
    Image(String),
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            FetchError::Timeout
        } else if e.is_connect() {
            FetchError::Unreachable
        } else {
            FetchError::Failed
        }
    }
}

/// Downloads the images; one per server (see [`ImageFetcher::shared`]), so
/// refreshes never overlap.
pub struct ImageFetcher {
    http: reqwest::Client,
    /// One refresh at a time: startup, the button and an import can overlap.
    refreshing: tokio::sync::Mutex<()>,
}

impl ImageFetcher {
    fn new() -> Result<Self, String> {
        // reqwest uses rustls with ring; the first install wins.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS
                    || !matches!(attempt.url().scheme(), "http" | "https")
                {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            refreshing: tokio::sync::Mutex::new(()),
        })
    }

    /// The server's fetcher.
    pub fn shared() -> Arc<ImageFetcher> {
        static SHARED: OnceLock<Arc<ImageFetcher>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                Arc::new(ImageFetcher::new().expect("an HTTP client for the image downloads"))
            })
            .clone()
    }

    /// Downloads `url` and checks it like an upload. Ignores the content
    /// type: a raw file on GitHub is `text/plain`, so the content tells.
    pub async fn fetch(&self, url: &str) -> Result<Logo, FetchError> {
        download(&self.http, url).await
    }

    /// Downloads the URL images of the current competition (only the image in
    /// `only` if given) and stores the results: the last good copy stays if
    /// a download fails, with the reason saved for the competition
    /// management. Never holds the database across a download, so a reload
    /// isn't blocked, and a result for an image that changed meanwhile is
    /// dropped.
    pub async fn refresh(&self, db: &ReloadableSqlite, only: Option<Slot>) -> sqlx::Result<()> {
        let _refreshing = self.refreshing.lock().await;
        let todo: Vec<Image> = {
            let pool = db.get().await.clone();
            images::list(&pool).await?
        }
        .into_iter()
        .filter(|image| image.source_url.is_some() && only.is_none_or(|slot| slot.is(image)))
        .collect();
        if todo.is_empty() {
            return Ok(());
        }

        let permits = Arc::new(Semaphore::new(PARALLEL));
        let mut downloads = JoinSet::new();
        for image in todo {
            let Some(url) = image.source_url else {
                continue;
            };
            let (http, permits) = (self.http.clone(), permits.clone());
            downloads.spawn(async move {
                let _permit = permits.acquire().await;
                let result = download(&http, &url).await;
                (image.id, image.kind, image.name, url, result)
            });
        }
        while let Some(done) = downloads.join_next().await {
            let Ok((id, kind, name, url, result)) = done else {
                continue;
            };
            let label = if name.is_empty() { kind } else { name };
            let result = match result {
                Ok(logo) => {
                    info!("Downloaded the image {label} from {url}");
                    Ok(logo)
                }
                Err(e) => {
                    warn!("Couldn't download the image {label} from {url}: {e}");
                    Err(e.to_string())
                }
            };
            let pool = db.get().await.clone();
            images::store_fetched(&pool, id, &url, result).await?;
        }
        Ok(())
    }

    /// Refreshes in the background, e.g. at startup, which must neither wait
    /// for the downloads nor fail on them.
    pub fn refresh_in_background(self: &Arc<Self>, db: ReloadableSqlite, only: Option<Slot>) {
        let fetcher = self.clone();
        tokio::spawn(async move {
            if let Err(e) = fetcher.refresh(&db, only).await {
                warn!("Couldn't refresh the images: {e}");
            }
        });
    }
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Logo, FetchError> {
    let mut response = http
        .get(url)
        .header(USER_AGENT, "einrad-server")
        .send()
        .await?;
    let status = response.status();
    if status != StatusCode::OK {
        return Err(FetchError::Status(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > logo::MAX_SIZE as u64)
    {
        return Err(FetchError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > logo::MAX_SIZE {
            return Err(FetchError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    // Scaling a raster image takes a moment.
    tokio::task::spawn_blocking(move || Logo::from_upload(&bytes))
        .await
        .map_err(|_| FetchError::Failed)?
        .map_err(|e| FetchError::Image(e.to_string()))
}
