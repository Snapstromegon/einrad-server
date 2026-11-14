//! The `competition_image` rows. Backend internals; the application goes
//! through [`db_core::MediaRepo`]. The functions are `pub` only for the seed,
//! demo and export code that still holds a connection (#238, step D8b).

use db_core::media::{Image, Logo, Slot, StoredLogo};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

/// All images of the current competition: logo, break image, then the
/// sponsors in order.
pub async fn list(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Vec<Image>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!: i64", kind, position, name, link, source_url, mime_type,
            data IS NOT NULL as "has_data!: bool",
            updated_at as "updated_at: OffsetDateTime",
            fetched_at as "fetched_at: OffsetDateTime", fetch_error
        FROM competition_image
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
        ORDER BY CASE kind WHEN 'logo' THEN 0 WHEN 'break' THEN 1 ELSE 2 END, position, id
        "#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Image {
            id: row.id,
            kind: row.kind,
            position: row.position,
            name: row.name,
            link: row.link,
            source_url: row.source_url,
            mime_type: row.mime_type,
            has_data: row.has_data,
            updated_at: row.updated_at,
            fetched_at: row.fetched_at,
            fetch_error: row.fetch_error,
        })
        .collect())
}

/// The id and last change of the image in `slot` if it has bytes, so the
/// ETag reads no image.
pub async fn meta(
    db: impl sqlx::SqliteExecutor<'_>,
    slot: Slot,
) -> sqlx::Result<Option<(i64, OffsetDateTime)>> {
    let (kind, id) = match slot {
        Slot::Sponsor(id) => ("sponsor", id),
        slot => (slot.kind(), -1),
    };
    let row = sqlx::query!(
        r#"
        SELECT id as "id!: i64", updated_at as "updated_at: OffsetDateTime"
        FROM competition_image
        WHERE competition_id = (SELECT id FROM competition ORDER BY start_date DESC LIMIT 1)
            AND data IS NOT NULL AND kind = $1 AND (kind != 'sponsor' OR id = $2)
        "#,
        kind,
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.id, row.updated_at)))
}

/// The bytes of the image row `id`; `None` without any.
pub async fn load(db: impl sqlx::SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<StoredLogo>> {
    let row = sqlx::query!(
        r#"
        SELECT mime_type, data, updated_at as "updated_at: OffsetDateTime"
        FROM competition_image
        WHERE id = $1 AND data IS NOT NULL
        "#,
        id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|row| {
        Some(StoredLogo {
            mime_type: row.mime_type?,
            data: row.data?,
            updated_at: row.updated_at,
        })
    }))
}

/// The bytes of the logo or the break image of the current competition;
/// `None` without any (a URL image that was never downloaded has none).
pub async fn load_single(
    db: &mut SqliteConnection,
    slot: Slot,
) -> sqlx::Result<Option<StoredLogo>> {
    match meta(&mut *db, slot).await? {
        Some((id, _)) => load(&mut *db, id).await,
        None => Ok(None),
    }
}

/// Sets the image in `slot` to `logo`, an inline SVG or an upload: it no
/// longer comes from a URL. `false` if a sponsor doesn't exist.
pub async fn set_data(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    slot: Slot,
    logo: &Logo,
) -> sqlx::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let mime_type = logo.format.mime_type();
    if let Slot::Sponsor(id) = slot {
        let result = sqlx::query!(
            r#"
            UPDATE competition_image SET
                source_url = NULL, mime_type = $1, data = $2, updated_at = $3,
                fetched_at = NULL, fetch_error = NULL
            WHERE id = $4 AND competition_id = $5 AND kind = 'sponsor'
            "#,
            mime_type,
            logo.data,
            now,
            id,
            competition_id,
        )
        .execute(db)
        .await?;
        return Ok(result.rows_affected() > 0);
    }
    let kind = slot.kind();
    sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, mime_type, data, updated_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (competition_id, kind) WHERE kind != 'sponsor' DO UPDATE SET
            source_url = NULL,
            mime_type = excluded.mime_type,
            data = excluded.data,
            updated_at = excluded.updated_at,
            fetched_at = NULL,
            fetch_error = NULL
        "#,
        competition_id,
        kind,
        mime_type,
        logo.data,
        now,
    )
    .execute(db)
    .await?;
    Ok(true)
}

/// Makes `url` the source of the image in `slot`. The old bytes stay (from
/// an SVG, an upload or another URL) until the first good download of `url`
/// replaces them, so a typo in the URL costs nothing; its warning shows
/// meanwhile. The same URL changes nothing. Nothing is downloaded here. `false` if
/// a sponsor doesn't exist.
pub async fn set_url(
    conn: &mut SqliteConnection,
    competition_id: &str,
    slot: Slot,
    url: &str,
) -> sqlx::Result<bool> {
    let now = OffsetDateTime::now_utc();
    if let Slot::Sponsor(id) = slot {
        let result = sqlx::query!(
            r#"
            UPDATE competition_image SET
                source_url = $1, fetched_at = NULL, fetch_error = NULL
            WHERE id = $2 AND competition_id = $3 AND kind = 'sponsor'
                AND source_url IS NOT $4
            "#,
            url,
            id,
            competition_id,
            url,
        )
        .execute(&mut *conn)
        .await?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }
        let exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM competition_image
            WHERE id = $1 AND competition_id = $2 AND kind = 'sponsor'"#,
            id,
            competition_id
        )
        .fetch_one(&mut *conn)
        .await?;
        return Ok(exists > 0);
    }
    let kind = slot.kind();
    sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, source_url, updated_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (competition_id, kind) WHERE kind != 'sponsor' DO UPDATE SET
            source_url = excluded.source_url,
            fetched_at = NULL,
            fetch_error = NULL
        WHERE competition_image.source_url IS NOT excluded.source_url
        "#,
        competition_id,
        kind,
        url,
        now,
    )
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

/// Removes the logo or break image (so its fallback applies again), or the
/// sponsor.
pub async fn remove(db: impl sqlx::SqliteExecutor<'_>, slot: Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Sponsor(id) => {
            sqlx::query!(
                "DELETE FROM competition_image WHERE id = $1 AND kind = 'sponsor'",
                id
            )
            .execute(db)
            .await?;
        }
        slot => {
            let kind = slot.kind();
            sqlx::query!("DELETE FROM competition_image WHERE kind = $1", kind)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

/// Adds a sponsor at the end of the list, without an image yet; returns its
/// id.
pub async fn add_sponsor(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    name: &str,
    link: Option<&str>,
) -> sqlx::Result<i64> {
    let now = OffsetDateTime::now_utc();
    let result = sqlx::query!(
        r#"
        INSERT INTO competition_image (competition_id, kind, position, name, link, updated_at)
        VALUES ($1, 'sponsor',
            (SELECT COALESCE(MAX(position) + 1, 0) FROM competition_image
                WHERE competition_id = $2 AND kind = 'sponsor'),
            $3, $4, $5)
        "#,
        competition_id,
        competition_id,
        name,
        link,
        now,
    )
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Changes a sponsor's name and link; `false` if it doesn't exist.
pub async fn update_sponsor(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    name: &str,
    link: Option<&str>,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "UPDATE competition_image SET name = $1, link = $2 WHERE id = $3 AND kind = 'sponsor'",
        name,
        link,
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Moves a sponsor to `position` in the list (counted from 0).
pub async fn set_position(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    position: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE competition_image SET position = $1 WHERE id = $2 AND kind = 'sponsor'",
        position,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Stores what a download of `url` for image `id` gave. `false` if the image
/// is gone or has another source by now (the admin changed it while the
/// download ran): the result is dropped.
///
/// A good download keeps `updated_at`, so the ETag stays, if the bytes
/// didn't change. A failed one keeps the last good copy.
pub async fn store_fetched(
    db: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    url: &str,
    result: Result<Logo, String>,
) -> sqlx::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let affected = match result {
        Ok(logo) => {
            let mime_type = logo.format.mime_type();
            sqlx::query!(
                r#"
                UPDATE competition_image SET
                    updated_at = CASE WHEN data IS $1 AND mime_type IS $2 THEN updated_at ELSE $3 END,
                    mime_type = $4, data = $5, fetched_at = $6, fetch_error = NULL
                WHERE id = $7 AND source_url = $8
                "#,
                logo.data,
                mime_type,
                now,
                mime_type,
                logo.data,
                now,
                id,
                url,
            )
            .execute(db)
            .await?
            .rows_affected()
        }
        Err(error) => sqlx::query!(
            r#"
            UPDATE competition_image SET fetched_at = $1, fetch_error = $2
            WHERE id = $3 AND source_url = $4
            "#,
            now,
            error,
            id,
            url,
        )
        .execute(db)
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}
