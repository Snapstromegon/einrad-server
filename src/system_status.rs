use axum::{extract::FromRequestParts, http::request::Parts};
use time::OffsetDateTime;
use tracing::error;

use crate::{
    competition::Competition, http_server::extractor::auth::Auth,
    reloadable_sqlite::ReloadableSqlite, roles::Permission,
};

#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct Capabilities {
    pub can_register: bool,
    pub can_create_club: bool,
    pub can_register_rider: bool,
    pub can_upload_music: bool,
    /// Clubs can add, change and remove judges.
    pub can_register_judge: bool,
}

#[derive(Debug, Clone)]
pub struct StatusOptions {
    pub start_register_date: OffsetDateTime,
    pub end_register_date: OffsetDateTime,
    pub end_music_upload_date: OffsetDateTime,
    /// End of the judge registration; `end_register_date` without one.
    pub end_judge_register_date: Option<OffsetDateTime>,
}

impl StatusOptions {
    /// Computes which actions are currently allowed based on the configured dates.
    ///
    /// Registration (users, clubs, riders) is possible between
    /// `start_register_date` and `end_register_date` (both inclusive); music
    /// uploads are possible until `end_music_upload_date`, judges from
    /// `start_register_date` until `end_judge_register_date`.
    ///
    /// ```
    /// use nrw_freestyle_cup_registration::system_status::StatusOptions;
    /// use time::{Duration, OffsetDateTime};
    ///
    /// let now = OffsetDateTime::now_utc();
    /// let options = StatusOptions {
    ///     start_register_date: now - Duration::days(1),
    ///     end_register_date: now + Duration::days(1),
    ///     end_music_upload_date: now - Duration::days(1),
    ///     end_judge_register_date: None,
    /// };
    ///
    /// let capabilities = options.get_system_status();
    /// assert!(capabilities.can_register);
    /// assert!(!capabilities.can_upload_music);
    /// assert!(capabilities.can_register_judge);
    /// ```
    pub fn get_system_status(&self) -> Capabilities {
        let now = OffsetDateTime::now_utc();

        let in_register_period = now >= self.start_register_date && now <= self.end_register_date;

        Capabilities {
            can_register: in_register_period,
            can_create_club: in_register_period,
            can_register_rider: in_register_period,
            can_upload_music: now <= self.end_music_upload_date,
            can_register_judge: now >= self.start_register_date
                && now
                    <= self
                        .end_judge_register_date
                        .unwrap_or(self.end_register_date),
        }
    }
}

impl Capabilities {
    /// Everything is closed, e.g. because no competition is configured.
    pub const CLOSED: Capabilities = Capabilities {
        can_register: false,
        can_create_club: false,
        can_register_rider: false,
        can_upload_music: false,
        can_register_judge: false,
    };
}

/// Who manages registrations can always do everything. For everyone else the dates of the
/// current competition decide.
impl<S> FromRequestParts<S> for Capabilities
where
    S: Send + Sync,
{
    type Rejection = ();
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Ok(auth) = Auth::from_request_parts(parts, state).await
            && auth.can(Permission::ManageRegistrations)
        {
            return Ok(Capabilities {
                can_register: true,
                can_create_club: true,
                can_register_rider: true,
                can_upload_music: true,
                can_register_judge: true,
            });
        }
        let db = parts.extensions.get::<ReloadableSqlite>().unwrap();
        let db = db.get().await.clone();
        match Competition::current(&db).await {
            Ok(Some(competition)) => Ok(competition.status_options().get_system_status()),
            Ok(None) => Ok(Capabilities::CLOSED),
            Err(e) => {
                error!("Couldn't load the competition: {e}");
                Ok(Capabilities::CLOSED)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    fn options(start: i64, end: i64, music_end: i64) -> StatusOptions {
        let now = OffsetDateTime::now_utc();
        StatusOptions {
            start_register_date: now + Duration::days(start),
            end_register_date: now + Duration::days(end),
            end_music_upload_date: now + Duration::days(music_end),
            end_judge_register_date: None,
        }
    }

    fn assert_registration(capabilities: Capabilities, open: bool) {
        assert_eq!(capabilities.can_register, open);
        assert_eq!(capabilities.can_create_club, open);
        assert_eq!(capabilities.can_register_rider, open);
    }

    #[test]
    fn registration_is_closed_before_start() {
        let capabilities = options(1, 10, 10).get_system_status();
        assert_registration(capabilities, false);
        assert!(capabilities.can_upload_music);
    }

    #[test]
    fn registration_is_open_within_period() {
        let capabilities = options(-1, 1, 1).get_system_status();
        assert_registration(capabilities, true);
        assert!(capabilities.can_upload_music);
    }

    #[test]
    fn registration_is_closed_after_end() {
        let capabilities = options(-10, -1, 5).get_system_status();
        assert_registration(capabilities, false);
        assert!(capabilities.can_upload_music);
    }

    #[test]
    fn music_upload_is_closed_after_deadline() {
        let capabilities = options(-10, -5, -1).get_system_status();
        assert_registration(capabilities, false);
        assert!(!capabilities.can_upload_music);
    }
}
