use db_core::{
    DbError, RidersRepo,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId},
    riders::{ClubRider, NewRider, PairRequest, Rider, RiderEdit, Starts},
};

use super::SqliteRepo;
use crate::registration::{
    self, PartnerClubs, act_access, club_access, ensure_rider_name_is_free, map_duplicate,
    rider_access,
};

#[async_trait::async_trait]
impl RidersRepo for SqliteRepo {
    async fn add_rider(
        &self,
        actor: &Actor,
        rider: &NewRider,
    ) -> Result<RiderId, RegistrationError> {
        let mut tx = self.begin_write().await?;
        club_access(&mut tx, actor, rider.club_id).await?;
        ensure_rider_name_is_free(
            &mut tx,
            rider.club_id,
            &rider.firstname,
            &rider.lastname,
            None,
        )
        .await?;

        let rider_id = RiderId::new();
        sqlx::query!(
            r#"
        INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender)
        VALUES ($1, $2, $3, $4, $5, $6);
        "#,
            rider_id,
            rider.club_id,
            rider.firstname,
            rider.lastname,
            rider.birthdate,
            rider.gender,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| map_duplicate(e, &rider.firstname, &rider.lastname))?;

        tx.commit().await?;
        Ok(rider_id)
    }

    async fn edit_rider(
        &self,
        actor: &Actor,
        rider: RiderId,
        edit: &RiderEdit,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let club_id = rider_access(&mut tx, actor, rider).await?;
        ensure_rider_name_is_free(
            &mut tx,
            club_id,
            &edit.firstname,
            &edit.lastname,
            Some(rider),
        )
        .await?;

        sqlx::query!(
            r#"
        UPDATE rider SET firstname = $1, lastname = $2, birthdate = $3, gender = COALESCE($4, gender)
        WHERE id = $5
        "#,
            edit.firstname,
            edit.lastname,
            edit.birthdate,
            edit.gender,
            rider,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| map_duplicate(e, &edit.firstname, &edit.lastname))?;

        tx.commit().await?;
        Ok(())
    }

    async fn delete_rider(&self, actor: &Actor, rider: RiderId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        rider_access(&mut tx, actor, rider).await?;
        registration::delete_rider(&mut tx, rider).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_riders(&self) -> Result<Vec<Rider>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_as!(
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
        .fetch_all(&pool)
        .await?)
    }

    async fn list_club_riders(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<ClubRider>, RegistrationError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        club_access(&mut conn, actor, club).await?;
        Ok(sqlx::query_as!(
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
        .await?)
    }

    async fn list_pair_requests(&self) -> Result<Vec<PairRequest>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_as!(
            PairRequest,
            r#"SELECT rider_id as "rider_id!: RiderId", partner_name FROM pair_requests"#
        )
        .fetch_all(&pool)
        .await?)
    }

    async fn list_club_pair_requests(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<PairRequest>, RegistrationError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        club_access(&mut conn, actor, club).await?;
        Ok(sqlx::query_as!(
            PairRequest,
            r#"
        SELECT pair_requests.rider_id as "rider_id!: RiderId", pair_requests.partner_name
        FROM pair_requests JOIN rider ON rider.id = pair_requests.rider_id
        WHERE rider.club_id = $1
        "#,
            club
        )
        .fetch_all(&mut *conn)
        .await?)
    }

    async fn rider_starts(&self, rider: RiderId) -> Result<Starts, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(registration::starts(&mut conn, rider).await?)
    }

    async fn set_single(
        &self,
        actor: &Actor,
        rider: RiderId,
        single: bool,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        rider_access(&mut tx, actor, rider).await?;
        registration::set_single(&mut tx, rider, single).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn request_pair(
        &self,
        actor: &Actor,
        rider: RiderId,
        partner_name: Option<&str>,
    ) -> Result<Option<RiderId>, RegistrationError> {
        let mut tx = self.begin_write().await?;
        rider_access(&mut tx, actor, rider).await?;
        let partner = registration::request_pair(&mut tx, rider, partner_name).await?;
        tx.commit().await?;
        Ok(partner)
    }

    async fn link_pair(
        &self,
        actor: &Actor,
        rider: RiderId,
        partner: RiderId,
    ) -> Result<ActId, RegistrationError> {
        let mut tx = self.begin_write().await?;
        rider_access(&mut tx, actor, rider).await?;
        let clubs = if actor.manage_registrations {
            PartnerClubs::Any
        } else {
            PartnerClubs::Same
        };
        let act_id = registration::link_pair(&mut tx, rider, partner, clubs).await?;
        tx.commit().await?;
        Ok(act_id)
    }

    async fn leave_pair(&self, actor: &Actor, rider: RiderId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        rider_access(&mut tx, actor, rider).await?;
        registration::leave_pair(&mut tx, rider).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn dissolve_pair(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        act_access(&mut tx, actor, act).await?;
        registration::dissolve_pair(&mut tx, act).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn replace_act_rider(
        &self,
        actor: &Actor,
        act: ActId,
        old: RiderId,
        new: RiderId,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;

        let withdrawn = sqlx::query_scalar!(
            r#"SELECT withdrawn_at IS NOT NULL as "withdrawn!: bool" FROM acts WHERE id = $1"#,
            act,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RegistrationError::NotFound)?;
        if withdrawn {
            return Err(RegistrationError::Rejected(
                "Die Kür ist abgemeldet und kann nicht mehr geändert werden.".to_string(),
            ));
        }

        let club_id = rider_access(&mut tx, actor, old).await?;
        let new_club_id = rider_access(&mut tx, actor, new).await?;
        if club_id != new_club_id && !actor.manage_registrations {
            return Err(RegistrationError::Rejected(
                "Der neue Fahrer muss aus demselben Verein sein.".to_string(),
            ));
        }

        let participants = sqlx::query_scalar!(
            r#"SELECT rider_id as "rider_id!: RiderId" FROM act_participants WHERE act_id = $1"#,
            act,
        )
        .fetch_all(&mut *tx)
        .await?;
        if !participants.contains(&old) {
            return Err(RegistrationError::NotFound);
        }
        if participants.contains(&new) {
            return Err(RegistrationError::Rejected(
                "Der Fahrer fährt diese Kür schon.".to_string(),
            ));
        }

        registration::replace_rider(&mut tx, act, old, new).await?;

        tx.commit().await?;
        Ok(())
    }
}
