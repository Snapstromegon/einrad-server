//! Raw fixtures for tests that need a state or a reading the `Db` trait does
//! not offer. Not part of the application.

use db_core::ids::ActId;
use db_sqlite::SqliteRepo;

/// Inserts an act that has started and not ended.
pub async fn insert_running_act(repo: &SqliteRepo, id: ActId) {
    sqlx::query(
        "INSERT INTO acts (id, name, act_type, started_at) VALUES ($1, '', 'single', CURRENT_TIMESTAMP)",
    )
    .bind(id)
    .execute(&repo.pool().await)
    .await
    .unwrap();
}

/// The act's raw `music_started_at`.
pub async fn music_started_at(repo: &SqliteRepo, id: ActId) -> Option<String> {
    sqlx::query_scalar("SELECT music_started_at FROM acts WHERE id = $1")
        .bind(id)
        .fetch_one(&repo.pool().await)
        .await
        .unwrap()
}

/// Seconds since the act's `music_started_at`.
pub async fn music_started_seconds_ago(repo: &SqliteRepo, id: ActId) -> f64 {
    sqlx::query_scalar(
        "SELECT (julianday('now') - julianday(music_started_at)) * 86400 FROM acts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&repo.pool().await)
    .await
    .unwrap()
}
