//! `CategoryCommands` for SQLite.

use db_core::{
    act_type::ActType,
    actor::Actor,
    category_ages::{StoredCategoryAges, birthday_bounds, stored_coverage_warnings},
    commands::category::{
        AddCategory, CategoryCommands, CategoryFields, DeleteCategory, EditCategory,
        MoveCategoryDown, MoveCategoryUp, MoveOutcome,
    },
    error::CommandError,
    heats::Discipline,
    ids::CategoryId,
    structure_rules,
};
use sqlx::SqliteConnection;
use time::Date;

use super::{SqliteRepo, write_tx};

/// Which way to move a category in the order.
#[derive(Clone, Copy)]
enum MoveDirection {
    Up,
    Down,
}

#[async_trait::async_trait]
impl CategoryCommands for SqliteRepo {
    async fn add_category(
        &self,
        _actor: &Actor,
        cmd: AddCategory,
    ) -> Result<(CategoryId, Vec<String>), CommandError> {
        Ok(write_tx!(self, tx, {
            add_category(&mut tx, &cmd.fields).await?
        }))
    }

    async fn edit_category(
        &self,
        _actor: &Actor,
        cmd: EditCategory,
    ) -> Result<Vec<String>, CommandError> {
        Ok(write_tx!(self, tx, {
            edit_category(&mut tx, cmd.category_id, &cmd.fields).await?
        }))
    }

    async fn delete_category(
        &self,
        _actor: &Actor,
        cmd: DeleteCategory,
    ) -> Result<bool, CommandError> {
        let mut tx = self.begin_write().await?;
        let deleted = delete_category(&mut tx, cmd.category_id).await?;
        // Only a delete commits, as before.
        if deleted {
            tx.commit().await?;
        }
        Ok(deleted)
    }

    async fn move_category_up(
        &self,
        _actor: &Actor,
        cmd: MoveCategoryUp,
    ) -> Result<MoveOutcome, CommandError> {
        self.move_category(cmd.category_id, MoveDirection::Up).await
    }

    async fn move_category_down(
        &self,
        _actor: &Actor,
        cmd: MoveCategoryDown,
    ) -> Result<MoveOutcome, CommandError> {
        self.move_category(cmd.category_id, MoveDirection::Down)
            .await
    }
}

impl SqliteRepo {
    async fn move_category(
        &self,
        id: CategoryId,
        direction: MoveDirection,
    ) -> Result<MoveOutcome, CommandError> {
        let mut tx = self.begin_write().await?;
        let outcome = move_category(&mut tx, id, direction).await?;
        // Only a move commits: a refusal leaves even the "goes to the end
        // first" step of a category without an order undone, as before.
        if outcome == MoveOutcome::Moved {
            tx.commit().await?;
        }
        Ok(outcome)
    }
}

/// The determining day of the current competition; `None` without one.
async fn determining_day(conn: &mut SqliteConnection) -> sqlx::Result<Option<Date>> {
    Ok(crate::competition::current(conn)
        .await?
        .map(|competition| competition.determining_day))
}

/// Rejects ages the import rejects, e.g. a `min_age` not below `under_age`.
async fn check_ages(
    conn: &mut SqliteConnection,
    fields: &CategoryFields,
) -> Result<(), CommandError> {
    let day = determining_day(conn).await?.ok_or(CommandError::NotFound)?;
    birthday_bounds(&fields.name, fields.min_age, fields.under_age, day)
        .map_err(CommandError::Rejected)?;
    Ok(())
}

/// Warnings about gaps and overlaps of the stored categories' ages: the
/// check the import runs, for edits in the admin UI. Empty without a
/// competition.
async fn coverage_warnings(conn: &mut SqliteConnection) -> Result<Vec<String>, CommandError> {
    let Some(day) = determining_day(conn).await? else {
        return Ok(vec![]);
    };
    let categories = sqlx::query!(
        r#"
        SELECT name as "name!", act_type as "act_type: ActType", is_single_male, min_age, under_age
        FROM categories ORDER BY "order" NULLS FIRST, name
        "#
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| StoredCategoryAges {
        name: row.name,
        act_type: row.act_type,
        single_male: row.is_single_male,
        min_age: row.min_age,
        under_age: row.under_age,
    })
    .collect::<Vec<_>>();
    stored_coverage_warnings(&categories, day).map_err(CommandError::Rejected)
}

/// A category in a heat keeps its act type and gender (HEAT-1): the heat's
/// other categories and its finals share them.
async fn check_heat(
    conn: &mut SqliteConnection,
    id: CategoryId,
    fields: &CategoryFields,
) -> Result<(), CommandError> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT heats.name as "heat!", categories.name as "name!", act_type as "act_type: ActType", is_single_male
        FROM categories JOIN heats ON heats.id = categories.heat_id
        WHERE categories.id = $1
        "#,
        id
    )
    .fetch_optional(conn)
    .await?
    else {
        return Ok(());
    };
    structure_rules::check_category_keeps_discipline(
        Some((
            &row.name,
            &row.heat,
            Discipline::new(row.act_type, row.is_single_male),
        )),
        Discipline::new(fields.act_type, fields.is_single_male),
    )
}

async fn add_category(
    conn: &mut SqliteConnection,
    fields: &CategoryFields,
) -> Result<(CategoryId, Vec<String>), CommandError> {
    check_ages(&mut *conn, fields).await?;
    let id = CategoryId::new();
    sqlx::query!(
        r#"
            INSERT INTO categories (
                id,
                name,
                description,
                min_age,
                under_age,
                act_type,
                is_single_male,
                einfahrzeit_seconds,
                act_duration_seconds,
                judge_duration_seconds,
                "order"
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                (SELECT COALESCE(MAX("order"), 0) + 1 FROM categories)
            )
            "#,
        id,
        fields.name,
        fields.description,
        fields.min_age,
        fields.under_age,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
    )
    .execute(&mut *conn)
    .await?;
    let warnings = coverage_warnings(&mut *conn).await?;
    Ok((id, warnings))
}

async fn edit_category(
    conn: &mut SqliteConnection,
    id: CategoryId,
    fields: &CategoryFields,
) -> Result<Vec<String>, CommandError> {
    check_ages(&mut *conn, fields).await?;
    check_heat(&mut *conn, id, fields).await?;
    // A rename is a plain update: everything refers to the category by its ID.
    let updated = sqlx::query!(
        r#"
            UPDATE categories
            SET
                name = $1,
                description = $2,
                min_age = $3,
                under_age = $4,
                act_type = $5,
                is_single_male = $6,
                einfahrzeit_seconds = $7,
                act_duration_seconds = $8,
                judge_duration_seconds = $9
            WHERE id = $10
            "#,
        fields.name,
        fields.description,
        fields.min_age,
        fields.under_age,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
        id,
    )
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(CommandError::NotFound);
    }
    let warnings = coverage_warnings(&mut *conn).await?;
    Ok(warnings)
}

async fn delete_category(
    conn: &mut SqliteConnection,
    id: CategoryId,
) -> Result<bool, CommandError> {
    // The timeplan entries that reference this category go with it.
    sqlx::query!("DELETE FROM timeplan WHERE category_id = $1", id)
        .execute(&mut *conn)
        .await?;
    let result = sqlx::query!("DELETE FROM categories WHERE id = $1", id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Ok(false);
    }
    Ok(true)
}

async fn move_category(
    conn: &mut SqliteConnection,
    id: CategoryId,
    direction: MoveDirection,
) -> Result<MoveOutcome, CommandError> {
    let Some(current) = sqlx::query!(r#"SELECT "order" FROM categories WHERE id = $1"#, id)
        .fetch_optional(&mut *conn)
        .await?
    else {
        return Ok(MoveOutcome::NotFound);
    };
    // A category without an order goes to the end first.
    let current_order = if let Some(order) = current.order {
        order
    } else {
        let max_order = sqlx::query!(r#"SELECT MAX("order") as max_order FROM categories"#)
            .fetch_one(&mut *conn)
            .await?
            .max_order
            .unwrap_or(0);
        let new_order = max_order + 1;
        sqlx::query!(
            r#"UPDATE categories SET "order" = $1 WHERE id = $2"#,
            new_order,
            id
        )
        .execute(&mut *conn)
        .await?;
        new_order
    };
    // The neighbour: the largest order below, or the smallest above.
    let neighbour = match direction {
        MoveDirection::Up => sqlx::query!(
            r#"
                    SELECT id as "id!: CategoryId", "order" FROM categories
                    WHERE "order" < $1
                    ORDER BY "order" DESC
                    LIMIT 1
                    "#,
            current_order
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(|row| (row.id, row.order)),
        MoveDirection::Down => sqlx::query!(
            r#"
                    SELECT id as "id!: CategoryId", "order" FROM categories
                    WHERE "order" > $1
                    ORDER BY "order" ASC
                    LIMIT 1
                    "#,
            current_order
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(|row| (row.id, row.order)),
    };
    let Some((neighbour_id, Some(neighbour_order))) = neighbour else {
        return Ok(MoveOutcome::AtEnd);
    };
    // A temporary negative order avoids constraint violations.
    let temp_order = -999_999;
    sqlx::query!(
        r#"UPDATE categories SET "order" = $1 WHERE id = $2"#,
        temp_order,
        id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"UPDATE categories SET "order" = $1 WHERE id = $2"#,
        current_order,
        neighbour_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"UPDATE categories SET "order" = $1 WHERE id = $2"#,
        neighbour_order,
        id
    )
    .execute(&mut *conn)
    .await?;
    Ok(MoveOutcome::Moved)
}
