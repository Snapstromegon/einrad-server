use db_core::{
    ActsRepo, DbError,
    act_type::ActType,
    acts::{
        ActParticipant, ActRecord, ActSong, NormalizationStatus, SongFiles, StartlistAct,
        StartlistActParticipant,
    },
    ids::{ActId, CategoryId, ClubId},
};

use super::SqliteRepo;

#[async_trait::async_trait]
impl ActsRepo for SqliteRepo {
    async fn list_acts(&self) -> Result<Vec<ActRecord>, DbError> {
        let pool = self.pool().await;
        let rows = sqlx::query!(
            r#"
            SELECT
                view_act.id as "id!: ActId",
                view_act."order" as "act_order",
                categories."order" as "category_order?",
                view_act.name,
                view_act.group_name,
                view_act.owner_club_id as "owner_club_id: ClubId",
                view_act.song_file,
                view_act.description,
                view_act.song_file_name,
                view_act.song_file_normalized,
                view_act.normalization_status as "normalization_status: NormalizationStatus",
                view_act.act_type as "act_type!: ActType",
                view_act.max_age,
                view_act.participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>",
                -- Withdrawn acts show no category: they don't start.
                CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category_id END as "category_id?: CategoryId",
                CAST(CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category END AS TEXT) as "category?: String",
                (
                    view_act.act_type IN ('group_small', 'group_large')
                    AND EXISTS (
                        SELECT 1 FROM act_participants own
                        JOIN act_participants theirs
                            ON theirs.rider_id = own.rider_id AND theirs.act_id != own.act_id
                        JOIN acts other ON other.id = theirs.act_id AND other.act_type = view_act.act_type
                        WHERE own.act_id = view_act.id
                    )
                ) as "type_conflict!: bool",
                view_act.song_checked,
                view_act.withdrawn_at IS NOT NULL as "withdrawn!: bool",
                view_act.fee_waiver
            FROM view_act LEFT JOIN categories
                ON view_act.category_id = categories.id AND view_act.withdrawn_at IS NULL
            ORDER BY categories."order" IS NULL, categories."order", view_act."order" ASC NULLS FIRST
            "#
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ActRecord {
                id: row.id,
                name: row.name,
                group_name: row.group_name,
                owner_club_id: row.owner_club_id,
                song_file: row.song_file,
                description: row.description,
                song_file_name: row.song_file_name,
                song_file_normalized: row.song_file_normalized,
                normalization_status: row.normalization_status,
                act_type: row.act_type,
                max_age: row.max_age,
                participants: row.participants.0,
                category_id: row.category_id,
                category: row.category,
                type_conflict: row.type_conflict,
                song_checked: row.song_checked,
                withdrawn: row.withdrawn,
                fee_waiver: row.fee_waiver,
                act_order: row.act_order,
                category_order: row.category_order,
            })
            .collect())
    }

    async fn find_act(&self, id: ActId) -> Result<Option<ActRecord>, DbError> {
        let pool = self.pool().await;
        let row = sqlx::query!(
            r#"
            SELECT
                view_act.id as "id!: ActId",
                view_act."order" as "act_order",
                categories."order" as "category_order?",
                view_act.name,
                view_act.group_name,
                view_act.owner_club_id as "owner_club_id: ClubId",
                view_act.song_file,
                view_act.description,
                view_act.song_file_name,
                view_act.song_file_normalized,
                view_act.normalization_status as "normalization_status: NormalizationStatus",
                view_act.act_type as "act_type!: ActType",
                view_act.max_age as "max_age: f64",
                view_act.participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>",
                -- Withdrawn acts show no category: they don't start.
                CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category_id END as "category_id?: CategoryId",
                CAST(CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category END AS TEXT) as "category?: String",
                (
                    view_act.act_type IN ('group_small', 'group_large')
                    AND EXISTS (
                        SELECT 1 FROM act_participants own
                        JOIN act_participants theirs
                            ON theirs.rider_id = own.rider_id AND theirs.act_id != own.act_id
                        JOIN acts other ON other.id = theirs.act_id AND other.act_type = view_act.act_type
                        WHERE own.act_id = view_act.id
                    )
                ) as "type_conflict!: bool",
                view_act.song_checked,
                view_act.withdrawn_at IS NOT NULL as "withdrawn!: bool",
                view_act.fee_waiver
            FROM view_act LEFT JOIN categories
                ON view_act.category_id = categories.id AND view_act.withdrawn_at IS NULL
            WHERE view_act.id = $1
            "#,
            id
        )
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|row| ActRecord {
            id: row.id,
            name: row.name,
            group_name: row.group_name,
            owner_club_id: row.owner_club_id,
            song_file: row.song_file,
            description: row.description,
            song_file_name: row.song_file_name,
            song_file_normalized: row.song_file_normalized,
            normalization_status: row.normalization_status,
            act_type: row.act_type,
            max_age: row.max_age,
            participants: row.participants.0,
            category_id: row.category_id,
            category: row.category,
            type_conflict: row.type_conflict,
            song_checked: row.song_checked,
            withdrawn: row.withdrawn,
            fee_waiver: row.fee_waiver,
            act_order: row.act_order,
            category_order: row.category_order,
        }))
    }

    async fn list_startlist(&self) -> Result<Vec<StartlistAct>, DbError> {
        let pool = self.pool().await;
        let rows = sqlx::query!(
            r#"
            SELECT
                view_act.id as "id!: ActId",
                view_act."order" as "act_order",
                categories."order" as "category_order",
                view_act.name,
                view_act.group_name,
                view_act.act_type as "act_type!: ActType",
                view_act.participants as "participants!: sqlx::types::Json<Vec<StartlistActParticipant>>",
                view_act.category_id as "category_id: CategoryId",
                view_act.category as "category?: String"
            FROM view_act JOIN categories ON view_act.category_id = categories.id
            WHERE view_act.withdrawn_at IS NULL
            ORDER BY categories."order" NULLS FIRST, view_act."order" ASC NULLS FIRST
            "#
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StartlistAct {
                id: row.id,
                name: row.name,
                group_name: row.group_name,
                act_type: row.act_type,
                participants: row.participants.0,
                category_id: row.category_id,
                category: row.category,
                act_order: row.act_order,
                category_order: row.category_order,
            })
            .collect())
    }

    async fn set_act_order(&self, id: ActId, order: Option<i64>) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            UPDATE acts
            SET "order" = $1
            WHERE id = $2
            "#,
            order,
            id,
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn set_song_checked(&self, id: ActId, checked: bool) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            UPDATE acts
            SET song_checked = $1
            WHERE id = $2
            "#,
            checked,
            id,
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn clear_act_song(&self, id: ActId) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let done = sqlx::query!(
            r#"
            UPDATE acts
            SET song_file = NULL, song_file_name = NULL, song_file_normalized = NULL,
                normalization_status = NULL, song_checked = FALSE
            WHERE id = $1
            "#,
            id
        )
        .execute(&pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    async fn save_act_song(
        &self,
        id: ActId,
        file_name: &str,
        song_file: &str,
        normalized: &str,
    ) -> Result<Option<String>, DbError> {
        let mut tx = self.begin_write().await?;
        let previous =
            sqlx::query_scalar!("SELECT song_file_normalized FROM acts WHERE id = $1", id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        sqlx::query!(
            r#"
            UPDATE acts
            SET song_file_name = $1, song_file = $2, song_checked = FALSE,
                song_file_normalized = $3, normalization_status = 'pending'
            WHERE id = $4
            "#,
            file_name,
            song_file,
            normalized,
            id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(previous)
    }

    async fn finish_normalization(
        &self,
        id: ActId,
        normalized: &str,
        status: NormalizationStatus,
    ) -> Result<bool, DbError> {
        let pool = self.pool().await;
        let done = sqlx::query!(
            r#"
            UPDATE acts SET normalization_status = $1
            WHERE id = $2 AND song_file_normalized = $3
            "#,
            status,
            id,
            normalized,
        )
        .execute(&pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    async fn list_songs_to_normalize(&self) -> Result<Vec<SongFiles>, DbError> {
        let pool = self.pool().await;
        let rows = sqlx::query!(
            r#"
            SELECT id as "id!: ActId", song_file as "song_file!", song_file_normalized
            FROM acts
            WHERE song_file IS NOT NULL
                AND (normalization_status IS NULL OR normalization_status = 'pending')
            "#
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SongFiles {
                act_id: row.id,
                song_file: row.song_file,
                song_file_normalized: row.song_file_normalized,
            })
            .collect())
    }

    async fn list_song_files(&self) -> Result<Vec<SongFiles>, DbError> {
        let pool = self.pool().await;
        let rows = sqlx::query!(
            r#"
            SELECT id as "id!: ActId", song_file as "song_file!", song_file_normalized
            FROM acts WHERE song_file IS NOT NULL
            "#
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SongFiles {
                act_id: row.id,
                song_file: row.song_file,
                song_file_normalized: row.song_file_normalized,
            })
            .collect())
    }

    async fn queue_normalization(&self, id: ActId, normalized: &str) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            "UPDATE acts SET song_file_normalized = $1, normalization_status = 'pending' WHERE id = $2",
            normalized,
            id
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn list_act_songs(&self) -> Result<Vec<ActSong>, DbError> {
        let pool = self.pool().await;
        Ok(crate::db_restore::act_songs(&pool).await?)
    }
}
