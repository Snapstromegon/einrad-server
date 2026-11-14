//! Prometheus metrics at `GET /metrics`.
//!
//! Counts that live in the database (registrations, acts, music, money, the
//! live timeplan) are read on every scrape, so they are never stale. Events
//! (HTTP requests, mails, normalization jobs) are counted where they happen.
//! The endpoint needs `Authorization: Bearer <metrics_token>` and doesn't
//! exist (404) while no token is configured.

use std::{
    sync::{LazyLock, atomic::AtomicU64},
    time::Instant,
};

use axum::{
    Extension,
    extract::{MatchedPath, Request},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use prometheus_client::{
    encoding::text::encode,
    metrics::{
        counter::Counter,
        family::Family,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
        info::Info,
    },
    registry::Registry,
};
use tracing::error;

use crate::{
    competition::Features,
    http_server::{HttpServerOptions, routes::query::predict_timeplan::predict},
    system_status::StatusOptions,
    timeplan::Audience,
};

type Labels = Vec<(String, String)>;
type GaugeFamily = Family<Labels, Gauge>;
type FloatGaugeFamily = Family<Labels, Gauge<f64, AtomicU64>>;
type HistogramFamily = Family<Labels, Histogram, fn() -> Histogram>;

fn labels<const N: usize>(pairs: [(&str, &str); N]) -> Labels {
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn request_histogram() -> Histogram {
    // 5 ms … about 20 s.
    Histogram::new(exponential_buckets(0.005, 2.0, 13))
}

/// All metrics of the app, registered once.
pub struct Metrics {
    registry: Registry,
    // Registration
    users: GaugeFamily,
    clubs: Gauge,
    riders: Gauge,
    riders_by_club: GaugeFamily,
    riders_checked_in: Gauge,
    registration_open: Gauge,
    music_upload_open: Gauge,
    registration_end: Gauge,
    // Acts and music
    acts: GaugeFamily,
    acts_by_category: GaugeFamily,
    group_riders: GaugeFamily,
    songs: GaugeFamily,
    song_normalization: GaugeFamily,
    // Money
    payments_received: Gauge<f64, AtomicU64>,
    fees_due: Gauge<f64, AtomicU64>,
    // Event day
    timeplan_delay: Gauge,
    timeplan_entries: GaugeFamily,
    acts_performed: Gauge,
    timeplan_predicted_end: FloatGaugeFamily,
    // Events
    http_requests: Family<Labels, Counter>,
    http_duration: HistogramFamily,
    mails: Family<Labels, Counter>,
    normalization_jobs: Family<Labels, Counter>,
    normalization_duration: Histogram,
}

impl Metrics {
    fn new() -> Self {
        let mut registry = Registry::with_prefix("einrad_server");
        let mut metrics = Self {
            registry: Registry::default(),
            users: GaugeFamily::default(),
            clubs: Gauge::default(),
            riders: Gauge::default(),
            riders_by_club: GaugeFamily::default(),
            riders_checked_in: Gauge::default(),
            registration_open: Gauge::default(),
            music_upload_open: Gauge::default(),
            registration_end: Gauge::default(),
            acts: GaugeFamily::default(),
            acts_by_category: GaugeFamily::default(),
            group_riders: GaugeFamily::default(),
            songs: GaugeFamily::default(),
            song_normalization: GaugeFamily::default(),
            payments_received: Gauge::default(),
            fees_due: Gauge::default(),
            timeplan_delay: Gauge::default(),
            timeplan_entries: GaugeFamily::default(),
            acts_performed: Gauge::default(),
            timeplan_predicted_end: FloatGaugeFamily::default(),
            http_requests: Family::default(),
            http_duration: HistogramFamily::new_with_constructor(request_histogram),
            mails: Family::default(),
            normalization_jobs: Family::default(),
            // 1 s … about 8.5 min.
            normalization_duration: Histogram::new(exponential_buckets(1.0, 2.0, 10)),
        };
        let m = &metrics;
        registry.register(
            "build",
            "Version of the running server",
            Info::new(labels([("version", env!("CARGO_PKG_VERSION"))])),
        );
        registry.register("users", "Registered users", m.users.clone());
        registry.register("clubs", "Registered clubs", m.clubs.clone());
        registry.register("riders", "Registered riders", m.riders.clone());
        registry.register(
            "riders_by_club",
            "Registered riders per club",
            m.riders_by_club.clone(),
        );
        registry.register(
            "riders_checked_in",
            "Riders checked in on competition day",
            m.riders_checked_in.clone(),
        );
        registry.register(
            "registration_open",
            "1 while clubs can register riders",
            m.registration_open.clone(),
        );
        registry.register(
            "music_upload_open",
            "1 while clubs can upload music",
            m.music_upload_open.clone(),
        );
        registry.register(
            "registration_end_timestamp_seconds",
            "End of the registration (Meldeschluss) as a Unix timestamp",
            m.registration_end.clone(),
        );
        registry.register(
            "acts",
            "Acts by type and state (categorized, uncategorized or withdrawn)",
            m.acts.clone(),
        );
        registry.register(
            "acts_by_category",
            "Acts per category",
            m.acts_by_category.clone(),
        );
        registry.register(
            "group_riders",
            "Group memberships by kind (regular or replacement)",
            m.group_riders.clone(),
        );
        registry.register(
            "songs",
            "Acts by music state (missing, uploaded or checked)",
            m.songs.clone(),
        );
        registry.register(
            "song_normalization",
            "Songs by normalization status",
            m.song_normalization.clone(),
        );
        registry.register(
            "payments_received_euros",
            "Sum of the payments of all clubs, refunds subtracted",
            m.payments_received.clone(),
        );
        registry.register(
            "fees_due_euros",
            "Sum of what all clubs owe (start fees and jury fee)",
            m.fees_due.clone(),
        );
        registry.register(
            "timeplan_delay_seconds",
            "Predicted delay of the next step of the timeplan (negative: ahead)",
            m.timeplan_delay.clone(),
        );
        registry.register(
            "timeplan_entries",
            "Timeplan entries by status",
            m.timeplan_entries.clone(),
        );
        registry.register(
            "acts_performed",
            "Acts that have ended",
            m.acts_performed.clone(),
        );
        registry.register(
            "timeplan_predicted_end_timestamp_seconds",
            "Predicted end of the timeplan as a Unix timestamp",
            m.timeplan_predicted_end.clone(),
        );
        registry.register(
            "http_requests",
            "HTTP requests by method, matched route and status",
            m.http_requests.clone(),
        );
        registry.register(
            "http_request_duration_seconds",
            "HTTP request durations by matched route",
            m.http_duration.clone(),
        );
        registry.register("mails", "Mails sent, by result", m.mails.clone());
        registry.register(
            "normalization_jobs",
            "Song normalization jobs, by result",
            m.normalization_jobs.clone(),
        );
        registry.register(
            "normalization_duration_seconds",
            "Duration of song normalization jobs",
            m.normalization_duration.clone(),
        );
        metrics.registry = registry;
        metrics
    }

    /// Counts a sent (or failed) mail.
    pub fn mail_sent(&self, ok: bool) {
        let result = if ok { "ok" } else { "error" };
        self.mails
            .get_or_create(&labels([("result", result)]))
            .inc();
    }

    /// Counts a finished normalization job.
    pub fn normalization_finished(&self, ok: bool, seconds: f64) {
        let result = if ok { "ok" } else { "error" };
        self.normalization_jobs
            .get_or_create(&labels([("result", result)]))
            .inc();
        self.normalization_duration.observe(seconds);
    }

    /// Reads the database counts into the gauges.
    async fn refresh(
        &self,
        repo: &dyn db_core::Db,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let set = |family: &GaugeFamily, rows: Vec<(Labels, i64)>| {
            family.clear();
            for (label_set, value) in rows {
                family.get_or_create(&label_set).set(value);
            }
        };

        let counts = repo.db_counts().await?;
        set(
            &self.users,
            counts
                .users
                .into_iter()
                .map(|(verified, count)| (labels([("verified", &verified.to_string())]), count))
                .collect(),
        );
        self.clubs.set(counts.clubs);
        self.riders.set(counts.riders);
        self.riders_checked_in.set(counts.riders_checked_in);
        set(
            &self.riders_by_club,
            counts
                .riders_by_club
                .into_iter()
                .map(|(club, count)| (labels([("club", &club)]), count))
                .collect(),
        );

        let competition = repo.current_competition().await?;
        let capabilities = competition
            .as_ref()
            .map(|c| StatusOptions::from(c).get_system_status());
        self.registration_open.set(
            capabilities
                .as_ref()
                .is_some_and(|c| c.can_register_rider)
                .into(),
        );
        self.music_upload_open
            .set(capabilities.is_some_and(|c| c.can_upload_music).into());
        self.registration_end.set(
            competition
                .as_ref()
                .map_or(0, |c| c.registration_end.unix_timestamp()),
        );

        set(
            &self.acts,
            counts
                .acts
                .into_iter()
                .map(|(act_type, state, count)| {
                    (labels([("act_type", &act_type), ("state", &state)]), count)
                })
                .collect(),
        );
        set(
            &self.acts_by_category,
            counts
                .acts_by_category
                .into_iter()
                .map(|(category, count)| (labels([("category", &category)]), count))
                .collect(),
        );
        set(
            &self.group_riders,
            counts
                .group_riders
                .into_iter()
                .map(|(replacement, count)| {
                    let kind = if replacement {
                        "replacement"
                    } else {
                        "regular"
                    };
                    (labels([("kind", kind)]), count)
                })
                .collect(),
        );
        set(
            &self.songs,
            counts
                .songs
                .into_iter()
                .map(|(state, count)| (labels([("state", &state)]), count))
                .collect(),
        );
        set(
            &self.song_normalization,
            counts
                .song_normalization
                .into_iter()
                .map(|(status, count)| (labels([("status", &status)]), count))
                .collect(),
        );

        let paid = repo.paid_by_club().await?.values().sum();
        self.payments_received.set(paid);
        self.fees_due.set(
            repo.fees_by_club(Features::of(competition.as_ref()).jury_registration)
                .await?
                .values()
                .map(|fees| fees.amount_due.total)
                .sum(),
        );

        self.acts_performed.set(counts.acts_performed);
        set(
            &self.timeplan_entries,
            counts
                .timeplan_entries
                .into_iter()
                .map(|(status, count)| (labels([("status", &status)]), count))
                .collect(),
        );
        // An incomplete timeplan can't be predicted; that isn't a scrape error.
        self.timeplan_predicted_end.clear();
        match predict(repo, Audience::OrganizerTeam).await {
            Ok(timeplan) => {
                self.timeplan_delay.set(timeplan.offset);
                if let Some(last) = timeplan.items.last() {
                    self.timeplan_predicted_end
                        .get_or_create(&Labels::new())
                        .set(last.predicted_end.unix_timestamp() as f64);
                }
            }
            Err(_) => {
                self.timeplan_delay.set(0);
            }
        }
        Ok(())
    }
}

static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// The app's metrics.
pub fn metrics() -> &'static Metrics {
    &METRICS
}

/// `GET /metrics`: the metrics in the OpenMetrics text format.
pub async fn metrics_handler(
    Extension(repo): Extension<db_core::DbHandle>,
    Extension(options): Extension<std::sync::Arc<HttpServerOptions>>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = options.metrics_token.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|given| constant_time_eq(given.as_bytes(), token.as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if let Err(e) = metrics().refresh(&*repo).await {
        error!("Couldn't read the metrics from the database: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut body = String::new();
    if let Err(e) = encode(&mut body, &metrics().registry) {
        error!("Couldn't encode the metrics: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (
        [(
            header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// Compares without leaking the length of the matching prefix.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Middleware counting requests and their durations by matched route.
pub async fn track_http(request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_string();
    let method = request.method().to_string();
    let start = Instant::now();
    let response = next.run(request).await;
    let m = metrics();
    m.http_requests
        .get_or_create(&labels([
            ("method", &method),
            ("route", &route),
            ("status", response.status().as_str()),
        ]))
        .inc();
    m.http_duration
        .get_or_create(&labels([("route", &route)]))
        .observe(start.elapsed().as_secs_f64());
    response
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn metrics_are_off_without_a_token() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let options = HttpServerOptions {
            bind_address: String::new(),
            base_url: String::new(),
            data_path: std::path::PathBuf::new(),
            reload_db_token: None,
            metrics_token: None,
            competition_file_id: None,
        };
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer x".parse().unwrap());

        let response = metrics_handler(
            Extension(db_sqlite::SqliteRepo::from_pool(db).into_handle()),
            Extension(Arc::new(options)),
            headers,
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn tokens_are_compared_completely() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
    }
}
