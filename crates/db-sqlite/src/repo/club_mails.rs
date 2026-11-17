use std::collections::HashMap;

use db_core::{
    BookingsRepo, ClubMailsRepo, DbError,
    act_type::ActType,
    club_mails::{ActWithoutMusic, ClubData, DeliveryStatus, RenderedMail, SentClubMail, SentMail},
    clubs::club_code,
    ids::{ClubId, ClubMailId, UserId},
};
use sqlx::SqlitePool;
use time::OffsetDateTime;
use tracing::warn;

use super::SqliteRepo;

#[async_trait::async_trait]
impl ClubMailsRepo for SqliteRepo {
    async fn load_mail_clubs(
        &self,
        ids: &[ClubId],
        jury_registration: bool,
    ) -> Result<Option<Vec<ClubData>>, DbError> {
        let pool = self.pool().await;
        let mut fees = crate::fees::fees(&pool, jury_registration, None).await?;
        let paid = self.paid_by_club().await?;
        let mut clubs = Vec::with_capacity(ids.len());
        for &id in ids {
            let row = sqlx::query!(
                r#"
                SELECT
                    clubs.name,
                    users.name as contact_name,
                    users.email as contact_email,
                    COALESCE(clubs.responsible_name, users.name) as "representative_name!: String",
                    COALESCE(clubs.responsible_email, users.email) as "representative_email!: String"
                FROM clubs
                JOIN users ON users.id = clubs.owner_id
                WHERE clubs.id = $1
                "#,
                id
            )
            .fetch_optional(&pool)
            .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let fees = fees.remove(&id).unwrap_or_default();
            clubs.push(ClubData {
                id,
                name: row.name,
                code: club_code(&id),
                contact_name: row.contact_name,
                contact_email: row.contact_email,
                representative_name: row.representative_name,
                representative_email: row.representative_email,
                amount_due: fees.amount_due.total,
                paid: paid.get(&id).copied().unwrap_or(0.0),
                missing_deployments: fees.jury.map_or(0, |jury| jury.missing),
                missing_music: acts_without_music(&pool, id).await?,
            });
        }
        Ok(Some(clubs))
    }

    async fn load_all_mail_clubs(&self, jury_registration: bool) -> Result<Vec<ClubData>, DbError> {
        let pool = self.pool().await;
        let ids = sqlx::query_scalar!(r#"SELECT id as "id!: ClubId" FROM clubs ORDER BY name"#)
            .fetch_all(&pool)
            .await?;
        Ok(self
            .load_mail_clubs(&ids, jury_registration)
            .await?
            .unwrap_or_default())
    }

    async fn mail_reply_to(&self) -> Result<Option<String>, DbError> {
        let pool = self.pool().await;
        let email = crate::competition::current(&pool)
            .await?
            .and_then(|competition| competition.contact_email);
        Ok(email.filter(|email| !email.trim().is_empty()))
    }

    async fn record_club_mail(
        &self,
        author_id: UserId,
        author_name: &str,
        subject: &str,
        body: &str,
        reply_to: Option<&str>,
        mails: Vec<RenderedMail>,
    ) -> Result<SentClubMail, DbError> {
        let id = ClubMailId::new();
        let sent_at = OffsetDateTime::now_utc();
        let mut tx = self.begin_write().await?;
        sqlx::query!(
            r#"
            INSERT INTO club_mails (id, sent_at, user_id, user_name, subject, body, reply_to)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            id,
            sent_at,
            author_id,
            author_name,
            subject,
            body,
            reply_to,
        )
        .execute(&mut *tx)
        .await?;
        let mut sent = Vec::with_capacity(mails.len());
        for mail in mails {
            let addresses = serde_json::to_string(&mail.to).expect("addresses serialize");
            let mail_id = sqlx::query_scalar!(
                r#"
                INSERT INTO club_mail_messages (mail_id, club_id, club_name, addresses, subject, body)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING id as "id!: i64"
                "#,
                id,
                mail.club_id,
                mail.club_name,
                addresses,
                mail.subject,
                mail.body,
            )
            .fetch_one(&mut *tx)
            .await?;
            sent.push(SentMail {
                id: mail_id,
                club_id: mail.club_id,
                club_name: mail.club_name,
                to: mail.to,
                subject: mail.subject,
                body: mail.body,
                status: DeliveryStatus::Pending,
                error: None,
            });
        }
        tx.commit().await?;
        Ok(SentClubMail {
            id,
            sent_at,
            sender_name: author_name.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            reply_to: reply_to.map(str::to_string),
            mails: sent,
        })
    }

    async fn set_mail_status(
        &self,
        mail_id: i64,
        status: DeliveryStatus,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            "UPDATE club_mail_messages SET status = $1, error = $2 WHERE id = $3",
            status,
            error,
            mail_id,
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn list_club_mails(&self) -> Result<Vec<SentClubMail>, DbError> {
        let pool = self.pool().await;
        let club_mails = sqlx::query!(
            r#"
            SELECT
                id as "id!: ClubMailId",
                sent_at as "sent_at!: OffsetDateTime",
                user_name,
                subject,
                body,
                reply_to
            FROM club_mails
            ORDER BY sent_at DESC, id DESC
            "#
        )
        .fetch_all(&pool)
        .await?;
        let mails = sqlx::query!(
            r#"
            SELECT
                id as "id!: i64",
                mail_id as "mail_id!: ClubMailId",
                club_id as "club_id: ClubId",
                club_name,
                addresses,
                subject,
                body,
                status as "status!: DeliveryStatus",
                error
            FROM club_mail_messages
            ORDER BY id
            "#
        )
        .fetch_all(&pool)
        .await?;
        let mut by_club_mail: HashMap<ClubMailId, Vec<SentMail>> = HashMap::new();
        for row in mails {
            by_club_mail.entry(row.mail_id).or_default().push(SentMail {
                id: row.id,
                club_id: row.club_id,
                club_name: row.club_name,
                to: serde_json::from_str(&row.addresses).unwrap_or_else(|error| {
                    warn!(
                        "Club mail {} has unreadable addresses: {error}",
                        row.mail_id
                    );
                    Vec::new()
                }),
                subject: row.subject,
                body: row.body,
                status: row.status,
                error: row.error,
            });
        }
        Ok(club_mails
            .into_iter()
            .map(|club_mail| SentClubMail {
                mails: by_club_mail.remove(&club_mail.id).unwrap_or_default(),
                id: club_mail.id,
                sent_at: club_mail.sent_at,
                sender_name: club_mail.user_name,
                subject: club_mail.subject,
                body: club_mail.body,
                reply_to: club_mail.reply_to,
            })
            .collect())
    }
}

/// The acts `club_id` looks after that have no music: its singles and pairs
/// and the groups it created, without withdrawn ones; by type, then name.
async fn acts_without_music(
    db: &SqlitePool,
    club_id: ClubId,
) -> sqlx::Result<Vec<ActWithoutMusic>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            acts.act_type as "act_type: ActType",
            acts.name as title,
            COALESCE(
                acts.group_name,
                (
                    SELECT group_concat(rider.firstname || ' ' || rider.lastname, ' & ')
                    FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    WHERE act_participants.act_id = acts.id
                ),
                ''
            ) as "name!: String"
        FROM acts
        WHERE acts.song_file IS NULL
            AND acts.withdrawn_at IS NULL
            AND CASE WHEN acts.act_type IN ('group_small', 'group_large')
                THEN acts.owner_club_id = ?1
                ELSE EXISTS (
                    SELECT 1 FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    WHERE act_participants.act_id = acts.id AND rider.club_id = ?1
                )
            END
        "#,
        club_id
    )
    .fetch_all(db)
    .await?;
    let mut acts: Vec<_> = rows
        .into_iter()
        .map(|row| {
            (
                row.act_type,
                ActWithoutMusic {
                    name: row.name,
                    act_type: row.act_type.german_name(),
                    title: row.title,
                },
            )
        })
        .collect();
    let order = [
        ActType::Single,
        ActType::Pair,
        ActType::GroupSmall,
        ActType::GroupLarge,
    ];
    acts.sort_by_key(|(act_type, act)| {
        (
            order.iter().position(|t| t == act_type),
            act.name.to_lowercase(),
        )
    });
    Ok(acts.into_iter().map(|(_, act)| act).collect())
}
