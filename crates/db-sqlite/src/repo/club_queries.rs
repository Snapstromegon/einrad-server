//! `ClubQueries` for SQLite: riders and pair requests. One read transaction
//! per query.

use db_core::{
    act_type::ActType,
    actor::Actor,
    acts::NormalizationStatus,
    competition::Features,
    error::{CommandError, QueryError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId, UserId},
    queries::club::{
        ClubActParticipantRecord, ClubActRecord, ClubActs, ClubDetails, ClubListEntry, ClubProfile,
        ClubQueries, ClubRider, ClubRow, ClubView, GetClub, ListClubActs, ListClubPairRequests,
        ListClubRiders, PairRequest, Rider,
    },
};
use sqlx::SqliteConnection;

use super::{
    SqliteRepo,
    bookings::{paid_by_club, payments_of_club},
    read_tx,
};
use crate::{fees::fees_in, registration::club_access};

#[async_trait::async_trait]
impl ClubQueries for SqliteRepo {
    async fn get_club(
        &self,
        actor: Option<&Actor>,
        query: GetClub,
    ) -> Result<ClubView, QueryError> {
        read_tx!(self, tx, { get_club(&mut tx, actor, query).await })
    }

    async fn list_clubs(&self, _actor: &Actor) -> Result<Vec<ClubListEntry>, QueryError> {
        read_tx!(self, tx, { Ok(list_clubs(&mut tx).await?) })
    }

    async fn list_club_acts(
        &self,
        actor: &Actor,
        query: ListClubActs,
    ) -> Result<ClubActs, QueryError> {
        read_tx!(self, tx, {
            club_access(&mut tx, actor, query.club_id).await?;
            let competition = crate::competition::current(&mut *tx).await?;
            Ok(ClubActs {
                moderation_input: Features::of(competition.as_ref()).moderation_input,
                acts: list_club_acts(&mut tx, query.club_id).await?,
            })
        })
    }

    async fn list_riders(&self, _actor: &Actor) -> Result<Vec<Rider>, QueryError> {
        Ok(read_tx!(self, tx, { list_riders(&mut tx).await? }))
    }

    async fn list_club_riders(
        &self,
        actor: &Actor,
        query: ListClubRiders,
    ) -> Result<Vec<ClubRider>, QueryError> {
        read_tx!(self, tx, {
            club_access(&mut tx, actor, query.club_id).await?;
            Ok(list_club_riders(&mut tx, query.club_id).await?)
        })
    }

    async fn list_pair_requests(&self, _actor: &Actor) -> Result<Vec<PairRequest>, QueryError> {
        Ok(read_tx!(self, tx, { list_pair_requests(&mut tx).await? }))
    }

    async fn list_club_pair_requests(
        &self,
        actor: &Actor,
        query: ListClubPairRequests,
    ) -> Result<Vec<PairRequest>, QueryError> {
        read_tx!(self, tx, {
            club_access(&mut tx, actor, query.club_id).await?;
            Ok(list_club_pair_requests(&mut tx, query.club_id).await?)
        })
    }
}

async fn list_riders(conn: &mut SqliteConnection) -> sqlx::Result<Vec<Rider>> {
    sqlx::query_as!(
            Rider,
            r#"
        SELECT
            rider.id as "id!: RiderId",
            rider.club_id as "club_id!: ClubId",
            club.name as "club_name!",
            rider.firstname,
            rider.lastname,
            rider.birthdate as "birthdate: time::OffsetDateTime",
            rider.gender as "gender: Gender",
            COALESCE(
              (
                julianday((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1))
                - julianday(rider.birthdate)
              ) / 365.2425,
              0.0
            ) as "age_on_competition: f64",
            rider.checked_in_at as "checked_in_at: time::OffsetDateTime",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.act_type IN ('group_small', 'group_large')
            ) as "group_count!: i64",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.fee_waiver IS NOT NULL
            ) as "waived_count!: i64",
            rider.fee_waiver IS NOT NULL as "fee_waived!: bool",
            rider.fee_waiver
        FROM rider
          LEFT JOIN clubs as club ON club.id = rider.club_id
        "#
        )
    .fetch_all(&mut *conn)
    .await
}

async fn list_club_riders(
    conn: &mut SqliteConnection,
    club: ClubId,
) -> sqlx::Result<Vec<ClubRider>> {
    sqlx::query_as!(
        ClubRider,
        r#"
        SELECT
            rider.id as "id!: RiderId",
            rider.club_id as "club_id!: ClubId",
            rider.firstname,
            rider.lastname,
            rider.birthdate,
            rider.gender as "gender: Gender",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.act_type IN ('group_small', 'group_large')
            ) as "group_count!: i64",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.fee_waiver IS NOT NULL
            ) as "waived_count!: i64",
            rider.fee_waiver IS NOT NULL as "fee_waived!: bool"
        FROM rider
        WHERE rider.club_id = $1
        "#,
        club
    )
    .fetch_all(&mut *conn)
    .await
}

async fn list_pair_requests(conn: &mut SqliteConnection) -> sqlx::Result<Vec<PairRequest>> {
    sqlx::query_as!(
        PairRequest,
        r#"SELECT rider_id as "rider_id!: RiderId", partner_name FROM pair_requests"#
    )
    .fetch_all(&mut *conn)
    .await
}

async fn list_club_pair_requests(
    conn: &mut SqliteConnection,
    club: ClubId,
) -> sqlx::Result<Vec<PairRequest>> {
    sqlx::query_as!(
        PairRequest,
        r#"
        SELECT pair_requests.rider_id as "rider_id!: RiderId", pair_requests.partner_name
        FROM pair_requests JOIN rider ON rider.id = pair_requests.rider_id
        WHERE rider.club_id = $1
        "#,
        club
    )
    .fetch_all(&mut *conn)
    .await
}

async fn get_club(
    conn: &mut SqliteConnection,
    actor: Option<&Actor>,
    query: GetClub,
) -> Result<ClubView, CommandError> {
    let profile = find_club(conn, query.club_id)
        .await?
        .ok_or(CommandError::NotFound)?;
    let details = if actor.is_some_and(|actor| actor.may_see_club_details(profile.id)) {
        let competition = crate::competition::current(&mut *conn).await?;
        let jury_registration = Features::of(competition.as_ref()).jury_registration;
        let fees = fees_in(conn, jury_registration, Some(profile.id))
            .await?
            .remove(&profile.id)
            .unwrap_or_default();
        let payments = payments_of_club(conn, profile.id).await?;
        Some(ClubDetails { payments, fees })
    } else {
        None
    };
    Ok(ClubView { profile, details })
}

async fn find_club(conn: &mut SqliteConnection, id: ClubId) -> sqlx::Result<Option<ClubProfile>> {
    let row = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: ClubId",
            clubs.name,
            clubs.owner_id as "owner_id: UserId",
            clubs.fee_waiver IS NOT NULL as "fee_waived!: bool",
            users.name as "owner_name?: String",
            users.email as "owner_email?: String",
            COALESCE(clubs.responsible_name, users.name) as "responsible_name?: String",
            COALESCE(clubs.responsible_email, users.email) as "responsible_email?: String"
        FROM clubs LEFT JOIN users ON users.id = clubs.owner_id
        WHERE clubs.id = $1
        "#,
        id
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| ClubProfile {
        id: row.id,
        name: row.name,
        owner_id: row.owner_id,
        fee_waived: row.fee_waived,
        owner_name: row.owner_name,
        owner_email: row.owner_email,
        responsible_name: row.responsible_name,
        responsible_email: row.responsible_email,
    }))
}

async fn list_clubs(conn: &mut SqliteConnection) -> sqlx::Result<Vec<ClubListEntry>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: ClubId",
            clubs.name,
            clubs.owner_id as "owner_id!: UserId",
            clubs.fee_waiver,
            users.name as owner_name,
            users.email as owner_email,
            users.email_verified as owner_email_verified,
            COALESCE(clubs.responsible_name, users.name) as "responsible_name!: String",
            COALESCE(clubs.responsible_email, users.email) as "responsible_email!: String"
        FROM clubs
        JOIN users ON users.id = clubs.owner_id
        ORDER BY clubs.name
        "#
    )
    .fetch_all(&mut *conn)
    .await?;
    let paid = paid_by_club(conn).await?;
    let competition = crate::competition::current(&mut *conn).await?;
    let jury_registration = Features::of(competition.as_ref()).jury_registration;
    let mut fees = fees_in(conn, jury_registration, None).await?;
    Ok(rows
        .into_iter()
        .map(|row| ClubListEntry {
            paid: paid.get(&row.id).copied().unwrap_or(0.0),
            fees: fees.remove(&row.id).unwrap_or_default(),
            club: ClubRow {
                id: row.id,
                name: row.name,
                owner_id: row.owner_id,
                fee_waiver: row.fee_waiver,
                owner_name: row.owner_name,
                owner_email: row.owner_email,
                owner_email_verified: row.owner_email_verified,
                responsible_name: row.responsible_name,
                responsible_email: row.responsible_email,
            },
        })
        .collect())
}

async fn list_club_acts(
    conn: &mut SqliteConnection,
    club_id: ClubId,
) -> sqlx::Result<Vec<ClubActRecord>> {
    let acts = sqlx::query!(
        r#"
        SELECT
            acts.id as "id!: ActId",
            acts.name,
            acts.group_name,
            acts.owner_club_id as "owner_club_id: ClubId",
            acts.description,
            acts.song_file_name,
            acts.song_file,
            acts.song_file_normalized,
            acts.normalization_status as "normalization_status: NormalizationStatus",
            acts.act_type as "act_type: ActType",
            acts.created_at as "created_at!: time::OffsetDateTime",
            acts.withdrawn_at IS NOT NULL as "withdrawn!: bool",
            acts.fee_waiver IS NOT NULL as "fee_waived!: bool"
        FROM acts
        WHERE acts.owner_club_id = ?1
            OR EXISTS (
                SELECT 1 FROM act_participants
                JOIN rider ON act_participants.rider_id = rider.id
                WHERE act_participants.act_id = acts.id AND rider.club_id = ?1
            )
        ORDER BY acts.id
        "#,
        club_id
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut records = Vec::with_capacity(acts.len());
    for act in acts {
        let participants = sqlx::query!(
            r#"
            SELECT
                rider.id as "id!: RiderId",
                rider.firstname,
                rider.lastname,
                rider.club_id as "club_id!: ClubId",
                clubs.name as club_name,
                act_participants.is_replacement,
                rider.birthdate
            FROM act_participants
            JOIN rider ON act_participants.rider_id = rider.id
            JOIN clubs ON clubs.id = rider.club_id
            WHERE act_participants.act_id = $1
            ORDER BY act_participants.is_replacement, rider.lastname, rider.firstname
            "#,
            act.id
        )
        .fetch_all(&mut *conn)
        .await?;
        records.push(ClubActRecord {
            id: act.id,
            name: act.name,
            group_name: act.group_name,
            owner_club_id: act.owner_club_id,
            description: act.description,
            song_file_name: act.song_file_name,
            song_file: act.song_file,
            song_file_normalized: act.song_file_normalized,
            normalization_status: act.normalization_status,
            act_type: act.act_type,
            created_at: act.created_at,
            withdrawn: act.withdrawn,
            fee_waived: act.fee_waived,
            participants: participants
                .into_iter()
                .map(|p| ClubActParticipantRecord {
                    id: p.id,
                    firstname: p.firstname,
                    lastname: p.lastname,
                    club_id: p.club_id,
                    club_name: p.club_name,
                    is_replacement: p.is_replacement,
                    birthdate: p.birthdate,
                })
                .collect(),
        });
    }
    Ok(records)
}
