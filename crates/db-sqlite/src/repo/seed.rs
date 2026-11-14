//! Seeding: the competition file (applied in one transaction), and the demo
//! data's rows. The file arrives as a checked [`CompetitionPlan`]; the SQL
//! only writes it (ADR 0005).

use std::collections::HashMap;

use db_core::{
    DbError, SeedRepo,
    ids::{CategoryId, FinalId, HeatId, JudgeId, JuryUnitId},
    jury::{JudgeTask, JuryUnitKind, RemapRegistration, unit_columns, unit_label},
    media::{Logo, Slot},
    seed::{
        Applied, ApplyOutcome, CompetitionPlan, Dropped, ImagePlan, Part, PlanBuilder,
        PreviewOutcome, SeedActDetails, SeedClub, SeedError, SeedGroup, SeedRider, SeedUser,
        SponsorImage, SponsorPlan, describe,
    },
};
use time::OffsetDateTime;
use tracing::{info, warn};

use super::SqliteRepo;
use crate::{competition, images, structure};

fn db(e: sqlx::Error) -> SeedError {
    SeedError::Db(DbError::Sqlx(e))
}

/// The message of a failed write, for the "invalid and ignored" warning.
fn message(e: &SeedError) -> String {
    match e {
        SeedError::Invalid(message) => message.clone(),
        SeedError::Db(e) => format!("Database error while applying the competition file: {e}"),
    }
}

#[async_trait::async_trait]
impl SeedRepo for SqliteRepo {
    async fn apply_competition_file(
        &self,
        file_id: &str,
        replace: bool,
        plan: PlanBuilder<'_>,
    ) -> Result<ApplyOutcome, SeedError> {
        let mut warnings = Vec::new();
        let mut tx = self.begin_write().await?;
        let mut dropped_registrations = Vec::new();
        let applied = match competition::current(&mut *tx).await.map_err(db)? {
            None => {
                let dropped = write(&mut tx, plan, file_id).await?;
                dropped_registrations = dropped.judge_registrations;
                info!("Created competition {file_id} from the competition file");
                Applied::Created
            }
            Some(current) if !replace && current.id != file_id => {
                warnings.push(format!(
                    "The competition file has the id {file_id}, the database's competition the id {}; \
                     the file is ignored. A competition's id never changes once created.",
                    current.id
                ));
                Applied::Ignored
            }
            Some(current) => {
                // Applied in a savepoint, so the comparison sees exactly what the
                // file would write.
                let before = Snapshot::take(&mut tx).await.map_err(db)?;
                let mut savepoint = sqlx::Connection::begin(&mut *tx).await.map_err(db)?;
                let written = write(&mut savepoint, plan, &current.id).await;
                let differences = match &written {
                    Ok(_) => before.differences(&Snapshot::take(&mut savepoint).await.map_err(db)?),
                    Err(_) => vec![],
                };
                match written {
                    Err(e) if !replace => {
                        warnings.push(format!(
                            "The competition file is invalid and ignored: {}",
                            message(&e)
                        ));
                        Applied::Ignored
                    }
                    Err(e) => return Err(e),
                    Ok(_) if differences.is_empty() => Applied::Unchanged,
                    Ok(_) if !replace => {
                        warnings.push(format!(
                            "The competition file differs from the database in {}; it is ignored, \
                             the database is the source of the competition. To apply it anyway, \
                             start with --replace-competition.",
                            describe(&differences)
                        ));
                        Applied::Ignored
                    }
                    Ok(dropped) => {
                        savepoint.commit().await.map_err(db)?;
                        if current.id != file_id {
                            warnings.push(format!(
                                "The competition file has the id {file_id}, the database's \
                                 competition the id {}; the replace keeps {}. A competition's id \
                                 never changes once created.",
                                current.id, current.id
                            ));
                        }
                        info!(
                            "Replaced {} of competition {} with the competition file",
                            describe(&differences),
                            current.id
                        );
                        dropped_registrations = dropped.judge_registrations.clone();
                        Applied::Replaced(dropped)
                    }
                }
            }
        };
        tx.commit().await.map_err(db)?;
        Ok(ApplyOutcome {
            applied,
            warnings,
            dropped_registrations,
        })
    }

    async fn preview_competition_file(
        &self,
        file_id: &str,
        plan: PlanBuilder<'_>,
    ) -> Result<PreviewOutcome, SeedError> {
        let mut tx = self.begin_write().await?;
        let current = competition::current(&mut *tx).await.map_err(db)?;
        let before = Snapshot::take(&mut tx).await.map_err(db)?;
        let id = current.as_ref().map_or(file_id, |c| c.id.as_str());
        let dropped = write(&mut tx, plan, id).await?;
        let changes = before.differences(&Snapshot::take(&mut tx).await.map_err(db)?);
        tx.rollback().await.map_err(db)?;
        Ok(PreviewOutcome {
            dropped: if current.is_some() && !changes.is_empty() {
                dropped
            } else {
                Dropped::default()
            },
            current_id: current.map(|c| c.id),
            changes,
        })
    }

    async fn create_competition(
        &self,
        file_id: &str,
        plan: PlanBuilder<'_>,
    ) -> Result<(), SeedError> {
        let mut tx = self.begin_write().await?;
        if competition::current(&mut *tx).await.map_err(db)?.is_some() {
            return Err(SeedError::Invalid(
                "Der Wettkampf lässt sich nicht anlegen, weil es schon einen Wettkampf gibt."
                    .to_string(),
            ));
        }
        write(&mut tx, plan, file_id).await?;
        tx.commit().await.map_err(db)?;
        info!("Created competition {file_id}");
        Ok(())
    }

    async fn seed_timeplan_progress(
        &self,
        progress: &[(Option<OffsetDateTime>, Option<OffsetDateTime>)],
    ) -> Result<(), DbError> {
        let pool = self.pool().await;
        let ids: Vec<i64> =
            sqlx::query_scalar!(r#"SELECT id as "id!" FROM timeplan ORDER BY position, id"#)
                .fetch_all(&pool)
                .await?;
        for (id, (started_at, ended_at)) in ids.iter().zip(progress) {
            sqlx::query!(
                "UPDATE timeplan SET started_at = $1, ended_at = $2 WHERE id = $3",
                started_at,
                ended_at,
                id
            )
            .execute(&pool)
            .await?;
        }
        Ok(())
    }

    async fn seed_user(&self, user: &SeedUser) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            INSERT INTO users (id, email, email_verified, name, password)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            user.id,
            user.email,
            user.email_verified,
            user.name,
            user.password_hash,
        )
        .execute(&pool)
        .await?;
        for role in &user.roles {
            sqlx::query!(
                "INSERT INTO user_roles (user_id, role) VALUES ($1, $2)",
                user.id,
                role
            )
            .execute(&pool)
            .await?;
        }
        if let Some(token) = user.verification_token {
            sqlx::query!(
                "INSERT INTO mail_verification (id, user_id, created_at) VALUES ($1, $2, $3)",
                token,
                user.id,
                user.now
            )
            .execute(&pool)
            .await?;
        }
        if let Some(token) = user.password_reset_token {
            sqlx::query!(
                "INSERT INTO password_resets (id, user_id, created_at) VALUES ($1, $2, $3)",
                token,
                user.id,
                user.now
            )
            .execute(&pool)
            .await?;
        }
        Ok(())
    }

    async fn seed_club(&self, club: &SeedClub) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            "INSERT INTO clubs (id, name, owner_id, fee_waiver) VALUES ($1, $2, $3, $4)",
            club.id,
            club.name,
            club.owner,
            club.fee_waiver,
        )
        .execute(&pool)
        .await?;
        if let Some(amount) = club.payment {
            let booking_id = db_core::ids::BookingId::new();
            sqlx::query!(
                "INSERT INTO bookings (id, amount, date, note, club_id) VALUES ($1, $2, $3, 'Überweisung', $4)",
                booking_id,
                amount,
                club.payment_date,
                club.id,
            )
            .execute(&pool)
            .await?;
        }
        sqlx::query!(
            "UPDATE users SET club_id = $1 WHERE id = $2",
            club.id,
            club.owner
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn seed_rider(&self, rider: &SeedRider) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            INSERT INTO rider (
                id, club_id, firstname, lastname, birthdate, gender, checked_in_at,
                fee_waiver
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            rider.id,
            rider.club_id,
            rider.firstname,
            rider.lastname,
            rider.birthdate,
            rider.gender,
            rider.checked_in_at,
            rider.fee_waiver,
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    async fn find_club_by_name(&self, name: &str) -> Result<Option<db_core::ids::ClubId>, DbError> {
        let pool = self.pool().await;
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: db_core::ids::ClubId" FROM clubs WHERE name = $1"#,
            name
        )
        .fetch_optional(&pool)
        .await?)
    }

    async fn seed_group(&self, group: &SeedGroup) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            INSERT INTO acts (id, name, act_type, group_name, owner_club_id)
            VALUES ($1, '', $2, $3, $4)
            "#,
            group.id,
            group.act_type,
            group.name,
            group.owner_club_id,
        )
        .execute(&pool)
        .await?;
        for (rider_id, is_replacement) in &group.participants {
            sqlx::query!(
                r#"
                INSERT INTO act_participants (act_id, rider_id, is_replacement)
                VALUES ($1, $2, $3)
                "#,
                group.id,
                rider_id,
                is_replacement,
            )
            .execute(&pool)
            .await?;
        }
        Ok(())
    }

    async fn seed_act_details(&self, details: &SeedActDetails) -> Result<(), DbError> {
        let pool = self.pool().await;
        sqlx::query!(
            r#"
            UPDATE acts SET
                name = $1, description = $2, song_file = $3, song_file_name = $4,
                song_checked = $5, "order" = $6, started_at = $7, ended_at = $8,
                fee_waiver = $9
            WHERE id = $10
            "#,
            details.name,
            details.description,
            details.song_file,
            details.song_file_name,
            details.song_checked,
            details.order,
            details.started_at,
            details.ended_at,
            details.fee_waiver,
            details.id,
        )
        .execute(&pool)
        .await?;
        Ok(())
    }
}

/// Writes the plan with competition `id` (`plan` builds it for that id):
/// the competition, its categories, heats, finals and timeplan, replacing
/// what the database has (ADR 0005). The planning that depends on them is
/// dropped, club data is kept. Judge registrations follow their heat or
/// final by name; the others are returned for the remap file.
async fn write(
    conn: &mut sqlx::SqliteConnection,
    plan: PlanBuilder<'_>,
    id: &str,
) -> Result<Dropped, SeedError> {
    let plan = plan(id).map_err(SeedError::Invalid)?;

    let mut dropped = Dropped {
        timeplan_slots: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM timeplan_slots"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?,
        act_progress: sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i64" FROM acts
            WHERE started_at IS NOT NULL OR ended_at IS NOT NULL OR music_started_at IS NOT NULL
            "#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?,
        finals_placements: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM final_slots WHERE act_id IS NOT NULL"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?,
        jury_assignments: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM jury_assignments"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?,
        jury_plan_published: sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM competition WHERE jury_plan_published_at IS NOT NULL"#
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?
            > 0,
        ..Default::default()
    };
    let registrations = sqlx::query!(
        r#"
        SELECT
            judge.id as "judge_id!: JudgeId",
            clubs.name as "club!",
            judge.firstname as "firstname!",
            judge.lastname as "lastname!",
            judge_registration.task as "task!: JudgeTask",
            view_jury_unit.kind as "unit_kind!: JuryUnitKind",
            view_jury_unit.name as "unit_name!"
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN clubs ON clubs.id = judge.club_id
        JOIN view_jury_unit ON view_jury_unit.id = judge_registration.unit_id
        ORDER BY clubs.name, judge.lastname, judge.firstname, view_jury_unit.kind DESC,
            view_jury_unit.name, judge_registration.task
        "#
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;

    for table in [
        "jury_assignments",
        "judge_registration",
        "timeplan_slots",
        "timeplan",
        "final_slots",
        "finals",
        "categories",
        "heats",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(&mut *conn)
            .await
            .map_err(db)?;
    }

    competition::upsert(&mut *conn, &plan.competition)
        .await
        .map_err(db)?;
    competition::write_colors(
        &mut *conn,
        &plan.competition.id,
        &plan.primary_color,
        &plan.secondary_color,
    )
    .await
    .map_err(db)?;
    // The app uses one competition. A row left over from an earlier id
    // would win in `Competition::current` if it starts later.
    sqlx::query!(
        "DELETE FROM competition WHERE id != $1",
        plan.competition.id
    )
    .execute(&mut *conn)
    .await
    .map_err(db)?;
    write_images(conn, &plan).await.map_err(db)?;
    sqlx::query!("UPDATE acts SET started_at = NULL, ended_at = NULL, music_started_at = NULL")
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    sqlx::query!("UPDATE competition SET jury_plan_published_at = NULL")
        .execute(&mut *conn)
        .await
        .map_err(db)?;

    for (index, category) in plan.categories.iter().enumerate() {
        let order = index as i64 + 1;
        let category_id = CategoryId::new();
        sqlx::query!(
            r#"
            INSERT INTO categories (
                id, name, description, min_age, under_age, act_type,
                is_single_male, "order", einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            category_id,
            category.name,
            category.description,
            category.min_age,
            category.under_age,
            category.act_type,
            category.single_male,
            order,
            category.einfahrzeit_seconds,
            category.act_duration_seconds,
            category.judge_duration_seconds,
        )
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    }
    if !plan.categories.is_empty() {
        info!(
            "Seeded {} categories from the competition file",
            plan.categories.len()
        );
    }

    // The file names heats and finals; the database refers to them by id.
    let mut heat_ids: HashMap<String, HeatId> = HashMap::new();
    for heat in &plan.heats {
        if heat.categories.is_empty() {
            warn!(
                "Heat {} has no categories; its finalist slots stay empty until an admin assigns them.",
                heat.name
            );
        }
        let heat_id = HeatId::new();
        heat_ids.insert(heat.code.clone(), heat_id);
        sqlx::query!(
            r#"
            INSERT INTO heats (
                id, name, code, description, threshold, skip, jury_registrable, jury_deployments,
                jury_needed_technical, jury_needed_performance, jury_needed_dismounts
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            heat_id,
            heat.name,
            heat.code,
            heat.description,
            heat.threshold,
            heat.skip,
            heat.jury.registrable,
            heat.jury.deployments,
            heat.jury.needed_technical,
            heat.jury.needed_performance,
            heat.jury.needed_dismounts,
        )
        .execute(&mut *conn)
        .await
        .map_err(db)?;
        structure::set_heat_categories(conn, heat_id, &heat.categories)
            .await
            .map_err(db)?;
    }
    if !plan.heats.is_empty() {
        info!(
            "Seeded {} heats from the competition file",
            plan.heats.len()
        );
    }

    for (order, item) in (1_i64..).zip(&plan.finals) {
        let final_id = FinalId::new();
        sqlx::query!(
            r#"
            INSERT INTO finals (
                id, name, code, description, act_type, is_single_male, "order",
                einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds,
                jury_registrable, jury_deployments, jury_needed_technical,
                jury_needed_performance, jury_needed_dismounts
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            final_id,
            item.name,
            item.code,
            item.description,
            item.act_type,
            item.single_male,
            order,
            item.einfahrzeit_seconds,
            item.act_duration_seconds,
            item.judge_duration_seconds,
            item.jury.registrable,
            item.jury.deployments,
            item.jury.needed_technical,
            item.jury.needed_performance,
            item.jury.needed_dismounts,
        )
        .execute(&mut *conn)
        .await
        .map_err(db)?;
        // The plan checked that every slot group's heat is in the file.
        let groups: Vec<db_core::finals::SlotGroupInput> = item
            .slots
            .iter()
            .map(|group| db_core::finals::SlotGroupInput {
                heat_id: heat_ids[&group.heat],
                count: group.count,
            })
            .collect();
        structure::replace_final_slots(conn, final_id, &groups)
            .await
            .map_err(db)?;
    }
    if !plan.finals.is_empty() {
        info!(
            "Seeded {} finals from the competition file",
            plan.finals.len()
        );
    }

    for (position, entry) in (1_i64..).zip(&plan.timeplan) {
        // The plan checked that the final exists.
        let final_id = match &entry.final_code {
            Some(code) => Some(
                sqlx::query_scalar!(
                    r#"SELECT id as "id: FinalId" FROM finals WHERE code = $1"#,
                    code
                )
                .fetch_one(&mut *conn)
                .await
                .map_err(db)?,
            ),
            None => None,
        };
        sqlx::query!(
            r#"
            INSERT INTO timeplan (
                earliest_start_time, duration_seconds, label, category_id, final_id, kind,
                show_photos, internal, position, einfahrzeit_seconds
            )
            VALUES ($1, $2, $3, (SELECT id FROM categories WHERE name = $4), $5, $6, $7, $8, $9, $10)
            "#,
            entry.earliest_start,
            entry.duration_seconds,
            entry.label,
            entry.category,
            final_id,
            entry.kind,
            entry.show_photos,
            entry.internal,
            position,
            entry.einfahrzeit_seconds,
        )
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    }
    if !plan.timeplan.is_empty() {
        info!(
            "Seeded {} timeplan entries from the competition file",
            plan.timeplan.len()
        );
    }

    for registration in registrations {
        let unit = sqlx::query_scalar!(
            r#"SELECT id as "id!: JuryUnitId" FROM view_jury_unit WHERE kind = $1 AND name = $2"#,
            registration.unit_kind,
            registration.unit_name
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
        match unit {
            Some(unit) => {
                let (heat_id, final_id) = unit_columns(registration.unit_kind, unit);
                sqlx::query!(
                    "INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)",
                    registration.judge_id,
                    heat_id,
                    final_id,
                    registration.task,
                )
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            }
            None => dropped.judge_registrations.push(RemapRegistration {
                club: registration.club,
                firstname: registration.firstname,
                lastname: registration.lastname,
                task: registration.task,
                old_jury_unit: unit_label(registration.unit_kind, &registration.unit_name),
                jury_unit: String::new(),
            }),
        }
    }
    Ok(dropped)
}

/// Applies the plan's logo, break image and sponsors: they're kept unless
/// the file sets them (ADR 0009, ADR 0010), and only written if they change,
/// so an unchanged file changes nothing and a download that's already there
/// stays.
async fn write_images(
    conn: &mut sqlx::SqliteConnection,
    plan: &CompetitionPlan,
) -> sqlx::Result<()> {
    let id = &plan.competition.id;
    write_single_image(conn, id, Slot::Logo, &plan.logo).await?;
    write_single_image(conn, id, Slot::Break, &plan.break_image).await?;
    if let Some(sponsors) = &plan.sponsors {
        write_sponsors(conn, id, sponsors).await?;
    }
    Ok(())
}

/// The logo or the break image: an SVG or a URL sets it, a reset removes it.
async fn write_single_image(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    slot: Slot,
    image: &ImagePlan,
) -> sqlx::Result<()> {
    match image {
        ImagePlan::Svg(svg) => {
            let current = images::list(&mut *conn)
                .await?
                .into_iter()
                .find(|image| slot.is(image));
            write_svg(conn, competition_id, slot, current.as_ref(), svg).await
        }
        ImagePlan::Url(url) => images::set_url(conn, competition_id, slot, url)
            .await
            .map(|_| ()),
        ImagePlan::Reset => images::remove(&mut *conn, slot).await,
        ImagePlan::Keep => Ok(()),
    }
}

/// Sets the image in `slot` to the inline `new` SVG, unless it already is.
async fn write_svg(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    slot: Slot,
    current: Option<&db_core::media::Image>,
    new: &Logo,
) -> sqlx::Result<()> {
    if let Some(current) = current.filter(|c| c.source_url.is_none() && c.has_data)
        && images::load(&mut *conn, current.id)
            .await?
            .is_some_and(|stored| stored.is(new))
    {
        return Ok(());
    }
    images::set_data(&mut *conn, competition_id, slot, new).await?;
    Ok(())
}

/// Makes the sponsors the file's list. A sponsor of the same name is the
/// same sponsor, so it keeps its image if the file gives none, and a list
/// that equals the database's changes nothing.
async fn write_sponsors(
    conn: &mut sqlx::SqliteConnection,
    competition_id: &str,
    sponsors: &[SponsorPlan],
) -> sqlx::Result<()> {
    let mut unused: Vec<db_core::media::Image> = images::list(&mut *conn)
        .await?
        .into_iter()
        .filter(|image| image.kind == "sponsor")
        .collect();
    for (position, sponsor) in (0..).zip(sponsors) {
        let name = &sponsor.name;
        let link = &sponsor.link;
        let current = unused
            .iter()
            .position(|image| &image.name == name)
            .map(|index| unused.remove(index));
        let id = match &current {
            Some(current) => {
                if &current.link != link {
                    images::update_sponsor(&mut *conn, current.id, name, link.as_deref()).await?;
                }
                if current.position != position {
                    images::set_position(&mut *conn, current.id, position).await?;
                }
                current.id
            }
            None => {
                let id =
                    images::add_sponsor(&mut *conn, competition_id, name, link.as_deref()).await?;
                images::set_position(&mut *conn, id, position).await?;
                id
            }
        };
        match &sponsor.image {
            Some(SponsorImage::Svg(svg)) => {
                write_svg(
                    conn,
                    competition_id,
                    Slot::Sponsor(id),
                    current.as_ref(),
                    svg,
                )
                .await?;
            }
            Some(SponsorImage::Url(url)) => {
                images::set_url(conn, competition_id, Slot::Sponsor(id), url).await?;
            }
            None => {}
        }
    }
    for gone in unused {
        images::remove(&mut *conn, Slot::Sponsor(gone.id)).await?;
    }
    Ok(())
}

/// The competition's settings and lists as the database has them, without
/// generated ids and progress, to tell whether a file changes anything.
#[derive(Debug, PartialEq)]
struct Snapshot {
    competition: Vec<String>,
    images: Vec<String>,
    colors: Vec<String>,
    categories: Vec<String>,
    heats: Vec<String>,
    finals: Vec<String>,
    timeplan: Vec<String>,
}

impl Snapshot {
    async fn take(conn: &mut sqlx::SqliteConnection) -> sqlx::Result<Self> {
        let mut rows = async |sql: &'static str| {
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *conn)
                .await
        };
        Ok(Self {
            competition: rows(
                r#"SELECT json_array(name, location, start_date, end_date, determining_day,
                    registration_start, registration_end, music_upload_end, fee_per_start,
                    max_fee_per_rider, venue, contact_email, announcement_url, photos_url,
                    payment_deadline, payment_account_holder, payment_iban, payment_bic,
                    payment_reference, jury_min_age, jury_deployments_per_two_acts,
                    jury_missing_deployment_fee, jury_registration_end,
                    feature_jury_registration, photos_source, photos_user, photos_password)
                FROM competition"#,
            )
            .await?,
            // `write` only touches an image if it changes; what a download
            // gave (fetched_at, fetch_error) isn't part of the file.
            images: rows(
                r#"SELECT json_array(kind, position, name, link, source_url, mime_type,
                    updated_at, data IS NOT NULL)
                FROM competition_image ORDER BY kind, position, id"#,
            )
            .await?,
            colors: rows(r#"SELECT json_array(primary_color, secondary_color) FROM competition"#)
                .await?,
            categories: rows(
                r#"SELECT json_array(name, description, min_age, under_age, act_type,
                    is_single_male, "order", einfahrzeit_seconds, act_duration_seconds,
                    judge_duration_seconds)
                FROM categories ORDER BY "order" NULLS FIRST, name"#,
            )
            .await?,
            heats: rows(
                r#"SELECT json_array(name, code, description, threshold, skip,
                    jury_registrable, jury_deployments, jury_needed_technical,
                    jury_needed_performance, jury_needed_dismounts,
                    (SELECT json_group_array(name)
                        FROM (SELECT name FROM categories WHERE heat_id = heats.id ORDER BY name)))
                FROM heats ORDER BY code"#,
            )
            .await?,
            finals: rows(
                r#"SELECT json_array(name, code, description, act_type, is_single_male, "order",
                    einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds,
                    jury_registrable, jury_deployments, jury_needed_technical,
                    jury_needed_performance, jury_needed_dismounts,
                    (SELECT json_group_array(json_array(heat, number))
                        FROM (SELECT heats.code AS heat, number FROM final_slots
                            JOIN heats ON heats.id = final_slots.heat_id
                            WHERE final_slots.final_id = finals.id ORDER BY position)))
                FROM finals ORDER BY "order", code"#,
            )
            .await?,
            timeplan: rows(
                r#"SELECT json_array(earliest_start_time, duration_seconds, label,
                    (SELECT name FROM categories WHERE categories.id = timeplan.category_id),
                    (SELECT code FROM finals WHERE finals.id = timeplan.final_id),
                    kind, show_photos, internal, einfahrzeit_seconds)
                FROM timeplan ORDER BY position, id"#,
            )
            .await?,
        })
    }

    /// The parts that differ from `other`.
    fn differences(&self, other: &Self) -> Vec<Part> {
        [
            (self.competition != other.competition, Part::Settings),
            (self.images != other.images, Part::Images),
            (self.colors != other.colors, Part::Colors),
            (self.categories != other.categories, Part::Categories),
            (self.heats != other.heats, Part::Heats),
            (self.finals != other.finals, Part::Finals),
            (self.timeplan != other.timeplan, Part::Timeplan),
        ]
        .into_iter()
        .filter_map(|(differs, what)| differs.then_some(what))
        .collect()
    }
}
