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
    competition::{Competition, Features},
    fees,
    http_server::{HttpServerOptions, routes::query::predict_timeplan::predict},
    reloadable_sqlite::ReloadableSqlite,
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
    async fn refresh(&self, db: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
        let set = |family: &GaugeFamily, rows: Vec<(Labels, i64)>| {
            family.clear();
            for (label_set, value) in rows {
                family.get_or_create(&label_set).set(value);
            }
        };

        let users = sqlx::query!(
            r#"SELECT email_verified as "verified!: bool", COUNT(*) as "count!: i64" FROM users GROUP BY email_verified"#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.users,
            users
                .into_iter()
                .map(|r| (labels([("verified", &r.verified.to_string())]), r.count))
                .collect(),
        );

        self.clubs.set(
            sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM clubs"#)
                .fetch_one(db)
                .await?,
        );
        self.riders.set(
            sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM rider"#)
                .fetch_one(db)
                .await?,
        );
        self.riders_checked_in.set(
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!: i64" FROM rider WHERE checked_in_at IS NOT NULL"#
            )
            .fetch_one(db)
            .await?,
        );
        let by_club = sqlx::query!(
            r#"
            SELECT clubs.name, COUNT(rider.id) as "count!: i64"
            FROM clubs LEFT JOIN rider ON rider.club_id = clubs.id
            GROUP BY clubs.id
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.riders_by_club,
            by_club
                .into_iter()
                .map(|r| (labels([("club", &r.name)]), r.count))
                .collect(),
        );

        let competition = Competition::current(db).await?;
        let capabilities = competition
            .as_ref()
            .map(|c| c.status_options().get_system_status());
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

        let acts = sqlx::query!(
            r#"
            SELECT
                act_type as "act_type!",
                CASE
                    WHEN withdrawn_at IS NOT NULL THEN 'withdrawn'
                    WHEN category IS NOT NULL THEN 'categorized'
                    ELSE 'uncategorized'
                END as "state!: String",
                COUNT(*) as "count!: i64"
            FROM view_act
            GROUP BY 1, 2
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.acts,
            acts.into_iter()
                .map(|r| {
                    (
                        labels([("act_type", &r.act_type), ("state", &r.state)]),
                        r.count,
                    )
                })
                .collect(),
        );
        let by_category = sqlx::query!(
            r#"
            SELECT category as "category!", COUNT(*) as "count!: i64"
            FROM view_act
            WHERE category IS NOT NULL AND withdrawn_at IS NULL
            GROUP BY category
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.acts_by_category,
            by_category
                .into_iter()
                .map(|r| (labels([("category", &r.category)]), r.count))
                .collect(),
        );
        let riders = sqlx::query!(
            r#"
            SELECT p.is_replacement as "replacement!: bool", COUNT(*) as "count!: i64"
            FROM act_participants p JOIN acts ON acts.id = p.act_id
            WHERE acts.act_type IN ('group_small', 'group_large')
            GROUP BY p.is_replacement
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.group_riders,
            riders
                .into_iter()
                .map(|r| {
                    let kind = if r.replacement {
                        "replacement"
                    } else {
                        "regular"
                    };
                    (labels([("kind", kind)]), r.count)
                })
                .collect(),
        );
        let songs = sqlx::query!(
            r#"
            SELECT
                CASE
                    WHEN song_file IS NULL THEN 'missing'
                    WHEN song_checked THEN 'checked'
                    ELSE 'uploaded'
                END as "state!: String",
                COUNT(*) as "count!: i64"
            FROM acts GROUP BY 1
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.songs,
            songs
                .into_iter()
                .map(|r| (labels([("state", &r.state)]), r.count))
                .collect(),
        );
        let normalization = sqlx::query!(
            r#"
            SELECT normalization_status as "status!", COUNT(*) as "count!: i64"
            FROM acts WHERE normalization_status IS NOT NULL GROUP BY 1
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.song_normalization,
            normalization
                .into_iter()
                .map(|r| (labels([("status", &r.status)]), r.count))
                .collect(),
        );

        let paid =
            sqlx::query_scalar!(r#"SELECT COALESCE(SUM(amount), 0.0) as "paid!: f64" FROM bookings WHERE club_id IS NOT NULL"#)
                .fetch_one(db)
                .await?;
        self.payments_received.set(paid);
        self.fees_due.set(
            fees::fees_by_club(
                db,
                competition.as_ref(),
                Features::of(competition.as_ref()).jury_registration,
            )
            .await?
            .values()
            .map(|fees| fees.amount_due.total)
            .sum(),
        );

        let performed = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM acts WHERE ended_at IS NOT NULL"#
        )
        .fetch_one(db)
        .await?;
        self.acts_performed.set(performed);
        let entries = sqlx::query!(
            r#"
            SELECT
                CASE
                    WHEN ended_at IS NOT NULL THEN 'ended'
                    WHEN started_at IS NOT NULL THEN 'started'
                    ELSE 'planned'
                END as "status!: String",
                COUNT(*) as "count!: i64"
            FROM timeplan GROUP BY 1
            "#
        )
        .fetch_all(db)
        .await?;
        set(
            &self.timeplan_entries,
            entries
                .into_iter()
                .map(|r| (labels([("status", &r.status)]), r.count))
                .collect(),
        );
        // An incomplete timeplan can't be predicted; that isn't a scrape error.
        self.timeplan_predicted_end.clear();
        match predict(db, Audience::OrganizerTeam).await {
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
    Extension(db): Extension<ReloadableSqlite>,
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

    let db = db.get().await.clone();
    if let Err(e) = metrics().refresh(&db).await {
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
            Extension(ReloadableSqlite::new(db, String::new())),
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
