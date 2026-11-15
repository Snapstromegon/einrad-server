//! `ClubQueries` for SQLite: riders and pair requests. One read transaction
//! per query.

use db_core::{
    actor::Actor,
    error::QueryError,
    gender::Gender,
    ids::{ClubId, RiderId},
    queries::club::{
        ClubQueries, ClubRider, ListClubPairRequests, ListClubRiders, PairRequest, Rider,
    },
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};
use crate::registration::club_access;

#[async_trait::async_trait]
impl ClubQueries for SqliteRepo {
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
