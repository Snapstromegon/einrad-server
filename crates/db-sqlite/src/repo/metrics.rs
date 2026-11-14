use db_core::{DbError, MetricsRepo, metrics::DbCounts};

use super::SqliteRepo;

#[async_trait::async_trait]
impl MetricsRepo for SqliteRepo {
    async fn db_counts(&self) -> Result<DbCounts, DbError> {
        let pool = self.pool().await;
        // One read transaction: all counts come from the same snapshot.
        let mut tx = pool.begin().await?;

        let users = sqlx::query!(
            r#"SELECT email_verified as "verified!: bool", COUNT(*) as "count!: i64" FROM users GROUP BY email_verified"#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.verified, r.count))
        .collect();
        let clubs = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM clubs"#)
            .fetch_one(&mut *tx)
            .await?;
        let riders = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM rider"#)
            .fetch_one(&mut *tx)
            .await?;
        let riders_checked_in = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM rider WHERE checked_in_at IS NOT NULL"#
        )
        .fetch_one(&mut *tx)
        .await?;
        let riders_by_club = sqlx::query!(
            r#"
            SELECT clubs.name, COUNT(rider.id) as "count!: i64"
            FROM clubs LEFT JOIN rider ON rider.club_id = clubs.id
            GROUP BY clubs.id
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.name, r.count))
        .collect();
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
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.act_type, r.state, r.count))
        .collect();
        let acts_by_category = sqlx::query!(
            r#"
            SELECT category as "category!", COUNT(*) as "count!: i64"
            FROM view_act
            WHERE category IS NOT NULL AND withdrawn_at IS NULL
            GROUP BY category
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.category, r.count))
        .collect();
        let group_riders = sqlx::query!(
            r#"
            SELECT p.is_replacement as "replacement!: bool", COUNT(*) as "count!: i64"
            FROM act_participants p JOIN acts ON acts.id = p.act_id
            WHERE acts.act_type IN ('group_small', 'group_large')
            GROUP BY p.is_replacement
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.replacement, r.count))
        .collect();
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
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.state, r.count))
        .collect();
        let song_normalization = sqlx::query!(
            r#"
            SELECT normalization_status as "status!", COUNT(*) as "count!: i64"
            FROM acts WHERE normalization_status IS NOT NULL GROUP BY 1
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.status, r.count))
        .collect();
        let acts_performed = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM acts WHERE ended_at IS NOT NULL"#
        )
        .fetch_one(&mut *tx)
        .await?;
        let timeplan_entries = sqlx::query!(
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
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.status, r.count))
        .collect();

        Ok(DbCounts {
            users,
            clubs,
            riders,
            riders_checked_in,
            riders_by_club,
            acts,
            acts_by_category,
            group_riders,
            songs,
            song_normalization,
            acts_performed,
            timeplan_entries,
        })
    }
}
