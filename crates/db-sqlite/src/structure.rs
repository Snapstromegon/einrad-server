//! The SQL helpers of categories, heats and finals on a connection: the
//! building blocks of one transaction behind [`db_core::CategoriesRepo`] and
//! [`db_core::HeatsFinalsRepo`].
//!
//! The helpers marked *transitional* are public because the competition
//! import (`seed`) and the timeplan handlers of the root crate still run
//! them inside their own transactions. They go away with those groups (#238).

use std::collections::HashMap;

use db_core::{
    act_type::ActType,
    clubs::RegistrationError,
    finals::SlotGroupInput,
    heats::{self, CategoryInfo, Discipline},
    ids::{ActId, CategoryId, FinalId, FinalSlotId, HeatId, JuryUnitId},
};
use sqlx::SqliteConnection;

/// The threshold for a new heat of `categories` without one (see
/// [`heats::default_threshold`]).
pub(crate) async fn default_threshold_of(
    conn: &mut SqliteConnection,
    categories: &[CategoryId],
) -> sqlx::Result<i64> {
    let mut under_ages = Vec::with_capacity(categories.len());
    for category in categories {
        under_ages.push(
            sqlx::query_scalar!("SELECT under_age FROM categories WHERE id = $1", category)
                .fetch_optional(&mut *conn)
                .await?
                .flatten(),
        );
    }
    Ok(heats::default_threshold(under_ages))
}

/// Whether an act of the heat, or its final (a slot or its block), has
/// started: then its Vorlauf can no longer be skipped or restored (Q5).
pub(crate) async fn heat_has_started(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"
        SELECT
            EXISTS (
                SELECT 1 FROM acts
                JOIN view_act ON view_act.id = acts.id
                JOIN categories ON categories.id = view_act.category_id
                WHERE categories.heat_id = ?1 AND acts.started_at IS NOT NULL
                    AND view_act.withdrawn_at IS NULL
            )
            OR EXISTS (
                SELECT 1 FROM final_slots
                WHERE final_id IN (SELECT final_id FROM final_slots WHERE heat_id = ?1)
                    AND started_at IS NOT NULL
            )
            OR EXISTS (
                SELECT 1 FROM timeplan
                WHERE final_id IN (SELECT final_id FROM final_slots WHERE heat_id = ?1)
                    AND started_at IS NOT NULL
            ) as "started!: bool"
        "#,
        heat_id
    )
    .fetch_one(conn)
    .await
}

/// The name of a heat, or `None` if there is none with that id.
pub(crate) async fn heat_name_of(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"SELECT name as "name!" FROM heats WHERE id = $1"#,
        heat_id
    )
    .fetch_optional(conn)
    .await
}

/// The short code of a heat, or `None` if there is none with that id.
/// *Transitional*: the timeplan handlers label slots with it.
pub async fn heat_code_of(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"SELECT code as "code!" FROM heats WHERE id = $1"#,
        heat_id
    )
    .fetch_optional(conn)
    .await
}

/// Checks that no other heat (not `except`) has the name or the code.
pub(crate) async fn check_heat_free(
    conn: &mut SqliteConnection,
    except: Option<HeatId>,
    name: &str,
    code: &str,
) -> Result<(), RegistrationError> {
    let name_taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM heats WHERE name = ?1 AND id IS NOT ?2) as "taken!: bool""#,
        name,
        except
    )
    .fetch_one(&mut *conn)
    .await?;
    if name_taken {
        return Err(RegistrationError::Conflict(format!(
            "Eine Vorlaufklasse „{name}“ gibt es schon."
        )));
    }
    let code_taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM heats WHERE code = ?1 AND id IS NOT ?2) as "taken!: bool""#,
        code,
        except
    )
    .fetch_one(&mut *conn)
    .await?;
    if code_taken {
        return Err(RegistrationError::Conflict(format!(
            "Eine Vorlaufklasse mit dem Kürzel „{code}“ gibt es schon."
        )));
    }
    Ok(())
}

/// The names of the categories with these IDs, in the order given and
/// without repeats. An ID no category has is a 404 with a German message
/// naming it.
pub(crate) async fn category_names(
    conn: &mut SqliteConnection,
    ids: &[CategoryId],
) -> Result<Vec<String>, RegistrationError> {
    let mut names: Vec<String> = vec![];
    for id in ids {
        let name = sqlx::query_scalar!(
            r#"SELECT name as "name!" FROM categories WHERE id = $1"#,
            id
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| {
            RegistrationError::NotFoundMessage(format!("Die Kategorie {id} gibt es nicht (mehr)."))
        })?;
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// The categories as far as heats care.
async fn category_infos(conn: &mut SqliteConnection) -> sqlx::Result<Vec<CategoryInfo>> {
    Ok(sqlx::query!(
        r#"
        SELECT categories.name as "name!", act_type as "act_type: ActType", is_single_male,
            heats.name as heat
        FROM categories
        LEFT JOIN heats ON heats.id = categories.heat_id
        ORDER BY categories."order" NULLS FIRST, categories.name
        "#
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| CategoryInfo {
        name: row.name,
        discipline: Discipline::new(row.act_type, row.is_single_male),
        heat: row.heat,
    })
    .collect())
}

/// The discipline of a heat, from its categories; `None` if it has none.
pub(crate) async fn heat_discipline(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
) -> sqlx::Result<Option<Discipline>> {
    Ok(sqlx::query!(
        r#"
        SELECT act_type as "act_type: ActType", is_single_male
        FROM categories WHERE heat_id = $1
        ORDER BY "order" NULLS FIRST LIMIT 1
        "#,
        heat_id
    )
    .fetch_optional(conn)
    .await?
    .map(|row| Discipline::new(row.act_type, row.is_single_male)))
}

/// The finals whose slots a heat fills, with their disciplines, by name.
pub(crate) async fn finals_of_heat(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
) -> sqlx::Result<Vec<(String, Discipline)>> {
    Ok(sqlx::query!(
        r#"
        SELECT DISTINCT finals.name as "name!", act_type as "act_type: ActType", is_single_male
        FROM finals JOIN final_slots ON final_slots.final_id = finals.id
        WHERE final_slots.heat_id = $1
        ORDER BY finals.name
        "#,
        heat_id
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| (row.name, Discipline::new(row.act_type, row.is_single_male)))
    .collect())
}

/// Sets the categories of a heat: the ones named join it, the others leave
/// it. The caller checks them first (see [`check_heat_categories`]).
/// *Transitional*: the competition import sets them in its own transaction.
pub async fn set_heat_categories(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
    categories: &[String],
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE categories SET heat_id = NULL WHERE heat_id = $1",
        heat_id
    )
    .execute(&mut *conn)
    .await?;
    for category in categories {
        sqlx::query!(
            "UPDATE categories SET heat_id = $1 WHERE name = $2",
            heat_id,
            category
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Checks the new categories of the heat `heat_id` (HEAT-1) and that they
/// keep the discipline of the finals it fills.
pub(crate) async fn check_heat_categories(
    conn: &mut SqliteConnection,
    heat_id: HeatId,
    categories: &[String],
) -> Result<(), RegistrationError> {
    let heat = heat_name_of(&mut *conn, heat_id)
        .await?
        .ok_or_else(|| RegistrationError::Rejected("Die Vorlaufklasse gibt es nicht.".into()))?;
    let known = category_infos(&mut *conn).await?;
    let Some(discipline) = heats::check(&heat, categories, &known)
        .map_err(|p| RegistrationError::Rejected(p.german()))?
    else {
        return Ok(());
    };
    let finals = finals_of_heat(conn, heat_id).await?;
    if let Some((name, _)) = finals.iter().find(|(_, d)| *d != discipline) {
        return Err(RegistrationError::Rejected(format!(
            "Die Vorlaufklasse füllt Finalplätze im Finale „{name}“; ihre Kategorien brauchen dessen Typ und Geschlecht."
        )));
    }
    Ok(())
}

/// Checks that no other final (not `except`) has the name or the code.
pub(crate) async fn check_final_free(
    conn: &mut SqliteConnection,
    except: Option<FinalId>,
    name: &str,
    code: &str,
) -> Result<(), RegistrationError> {
    let name_taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM finals WHERE name = ?1 AND id IS NOT ?2) as "taken!: bool""#,
        name,
        except
    )
    .fetch_one(&mut *conn)
    .await?;
    if name_taken {
        return Err(RegistrationError::Conflict(format!(
            "Ein Finale „{name}“ gibt es schon."
        )));
    }
    let code_taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM finals WHERE code = ?1 AND id IS NOT ?2) as "taken!: bool""#,
        code,
        except
    )
    .fetch_one(&mut *conn)
    .await?;
    if code_taken {
        return Err(RegistrationError::Conflict(format!(
            "Ein Finale mit dem Kürzel „{code}“ gibt es schon."
        )));
    }
    Ok(())
}

/// Replaces the slots of a final with new ones for `groups`, in order. A
/// slot keeps its placed act if the new slots have one with the same heat
/// and number. The caller checks that none of the old ones has started and
/// that the heats exist. *Transitional*: the competition import replaces
/// them in its own transaction.
pub async fn replace_final_slots(
    conn: &mut SqliteConnection,
    final_id: FinalId,
    groups: &[SlotGroupInput],
) -> sqlx::Result<()> {
    let placed: HashMap<(HeatId, i64), ActId> = sqlx::query!(
        r#"
        SELECT heat_id as "heat_id!: HeatId", number, act_id as "act_id!: ActId"
        FROM final_slots
        WHERE final_id = $1 AND act_id IS NOT NULL
        "#,
        final_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| ((row.heat_id, row.number), row.act_id))
    .collect();
    sqlx::query!("DELETE FROM final_slots WHERE final_id = $1", final_id)
        .execute(&mut *conn)
        .await?;
    let mut position = 0_i64;
    for group in groups {
        for number in 1..=group.count {
            position += 1;
            let id = FinalSlotId::new();
            let act_id = placed.get(&(group.heat_id, number));
            sqlx::query!(
                r#"
                INSERT INTO final_slots (id, final_id, position, heat_id, number, act_id)
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
                id,
                final_id,
                position,
                group.heat_id,
                number,
                act_id,
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// Checks that the heats of the final `final_id`'s slot groups exist, share
/// its discipline (an empty heat fits any) and fill no other final.
pub(crate) async fn check_final_heats(
    conn: &mut SqliteConnection,
    final_id: FinalId,
    act_type: ActType,
    is_single_male: bool,
    groups: &[SlotGroupInput],
) -> Result<(), RegistrationError> {
    let discipline = Discipline::new(act_type, is_single_male);
    for group in groups {
        let Some(heat) = heat_name_of(&mut *conn, group.heat_id).await? else {
            return Err(RegistrationError::Rejected(
                "Die Vorlaufklasse gibt es nicht.".to_string(),
            ));
        };
        let heat_discipline = heat_discipline(&mut *conn, group.heat_id).await?;
        if heat_discipline.is_some_and(|heat| heat != discipline) {
            return Err(RegistrationError::Rejected(format!(
                "Die Vorlaufklasse „{heat}“ hat einen anderen Typ oder ein anderes Geschlecht als das Finale."
            )));
        }
        let other = sqlx::query_scalar!(
            r#"
            SELECT finals.name as "name!" FROM final_slots
            JOIN finals ON finals.id = final_slots.final_id
            WHERE final_slots.heat_id = $1 AND final_slots.final_id != $2 LIMIT 1
            "#,
            group.heat_id,
            final_id
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(other) = other {
            return Err(RegistrationError::Rejected(format!(
                "Die Vorlaufklasse „{heat}“ füllt schon Finalplätze im Finale „{other}“; eine Vorlaufklasse füllt höchstens ein Finale."
            )));
        }
    }
    Ok(())
}

/// Whether the final has started: a slot, its timeplan block, or one of its
/// direct acts (an act of a skipped heat that fills it). A final that runs
/// only direct acts (group finals, skipped heats) has no started slot.
pub(crate) async fn final_has_started(
    conn: &mut SqliteConnection,
    final_id: FinalId,
) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"
        SELECT
            EXISTS (
                SELECT 1 FROM final_slots WHERE final_id = ?1 AND started_at IS NOT NULL
            )
            OR EXISTS (
                SELECT 1 FROM timeplan WHERE final_id = ?1 AND started_at IS NOT NULL
            )
            OR EXISTS (
                SELECT 1 FROM acts
                JOIN view_act ON view_act.id = acts.id
                JOIN categories ON categories.id = view_act.category_id
                JOIN heats ON heats.id = categories.heat_id
                WHERE heats.skip AND acts.started_at IS NOT NULL
                    AND view_act.withdrawn_at IS NULL
                    AND heats.id IN (SELECT heat_id FROM final_slots WHERE final_id = ?1)
            ) as "started!: bool"
        "#,
        final_id
    )
    .fetch_one(conn)
    .await
}

/// The name of a final, or `None` if there is none with that id.
/// *Transitional*: the timeplan handlers label blocks with it.
pub async fn final_name_of(
    conn: &mut SqliteConnection,
    final_id: FinalId,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"SELECT name as "name!" FROM finals WHERE id = $1"#,
        final_id
    )
    .fetch_optional(conn)
    .await
}

/// Checks that a final exists and runs in no timeplan block other than
/// `except` (the block being edited). The message is German.
/// *Transitional*: the timeplan handlers check it before they add a block.
pub async fn check_final_plannable(
    conn: &mut SqliteConnection,
    final_id: FinalId,
    except: Option<i64>,
) -> Result<(), String> {
    let Some(final_name) = final_name_of(&mut *conn, final_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("Das Finale gibt es nicht.".to_string());
    };
    let planned = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM timeplan WHERE final_id = $1 AND id IS NOT $2
        ) as "planned!: bool"
        "#,
        final_id,
        except
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    if planned {
        return Err(format!(
            "Das Finale „{final_name}“ ist schon im Zeitplan; jedes Finale läuft in einem Block."
        ));
    }
    Ok(())
}

/// Refuses (409, German message) to delete the heat or final `id` while
/// judges are registered or assigned to its jury unit (D4). The jury group
/// may promote it.
pub(crate) async fn ensure_jury_unit_unused(
    conn: &mut SqliteConnection,
    kind: &str,
    id: JuryUnitId,
) -> Result<(), RegistrationError> {
    let used = sqlx::query_scalar!(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM judge_registration r JOIN view_jury_unit u ON u.id = r.unit_id
                WHERE u.kind = ?1 AND u.id = ?2)
            + (SELECT COUNT(*) FROM jury_assignments a JOIN view_jury_unit u ON u.id = a.unit_id
                WHERE u.kind = ?1 AND u.id = ?2)
        ) as "count!: i64"
        "#,
        kind,
        id
    )
    .fetch_one(conn)
    .await?;
    if used > 0 {
        return Err(RegistrationError::Conflict(
            "Für die Jury dieser Vorlaufklasse oder dieses Finales sind JurorInnen gemeldet oder eingeteilt. Erst das ändern, dann löschen."
                .to_string(),
        ));
    }
    Ok(())
}
