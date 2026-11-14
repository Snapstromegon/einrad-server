//! Raw access for tests of other crates that need a state or a reading the
//! trait does not offer. Not part of the application.

use db_core::ids::ActId;

use super::SqliteRepo;

impl SqliteRepo {
    /// Inserts an act that has started and not ended.
    #[doc(hidden)]
    pub async fn insert_running_act_for_test(&self, id: ActId) {
        let pool = self.pool().await;
        sqlx::query(
            "INSERT INTO acts (id, name, act_type, started_at) VALUES ($1, '', 'single', CURRENT_TIMESTAMP)",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }

    /// The act's raw `music_started_at`.
    #[doc(hidden)]
    pub async fn music_started_at_for_test(&self, id: ActId) -> Option<String> {
        let pool = self.pool().await;
        sqlx::query_scalar("SELECT music_started_at FROM acts WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
    }

    /// Seconds since the act's `music_started_at`.
    #[doc(hidden)]
    pub async fn music_started_seconds_ago_for_test(&self, id: ActId) -> f64 {
        let pool = self.pool().await;
        sqlx::query_scalar(
            "SELECT (julianday('now') - julianday(music_started_at)) * 86400 FROM acts WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
    }
}
