//! The SQL behind the timeplan: loading it with every block's content and
//! the step-wise writes of the live timeplan. The rules and types are in
//! `db_core::timeplan`; the helpers here work inside one transaction opened
//! by a `TimeplanRepo` method (`repo/timeplan.rs`).

use std::collections::{HashMap, HashSet};

use db_core::{
    ids::{ActId, CategoryId, FinalId, FinalSlotId, HeatId},
    timeplan::{
        ActSlot, Audience, Block, BreakSlot, CategoryInfo, Entry, EntryKind, FinalistSlot,
        INTERNAL_PUBLIC_LABEL, Slot, Timeplan, effective_warmups, home_blocks,
    },
};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

/// Loads the whole timeplan for the organizer team (see [`load_for`]).
#[allow(dead_code)]
pub(crate) async fn load(db: &mut SqliteConnection) -> sqlx::Result<Timeplan> {
    load_for(db, Audience::OrganizerTeam).await
}

/// Loads the timeplan in order, with every block's slots, emptiness and
/// effective warm-ups, as `audience` may see it.
pub(crate) async fn load_for(
    db: &mut SqliteConnection,
    audience: Audience,
) -> sqlx::Result<Timeplan> {
    let public = audience == Audience::Public;
    let mut category_order: HashMap<CategoryId, i64> = HashMap::new();
    let categories: HashMap<CategoryId, CategoryInfo> = sqlx::query!(
        r#"
        SELECT
            id as "id!: CategoryId",
            name as "name!",
            description,
            "order",
            einfahrzeit_seconds,
            act_duration_seconds,
            judge_duration_seconds
        FROM categories
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| {
        category_order.insert(row.id, row.order.unwrap_or(i64::MAX));
        (
            row.id,
            CategoryInfo {
                description: row.description.unwrap_or_else(|| row.name.clone()),
                einfahrzeit_seconds: row.einfahrzeit_seconds,
                act_duration_seconds: row.act_duration_seconds,
                judge_duration_seconds: row.judge_duration_seconds,
            },
        )
    })
    .collect();

    let finals: HashMap<FinalId, CategoryInfo> = sqlx::query!(
        r#"
        SELECT
            id as "id!: FinalId",
            name as "name!",
            einfahrzeit_seconds,
            act_duration_seconds,
            judge_duration_seconds
        FROM finals
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.id,
            CategoryInfo {
                // A final shows its readable name.
                description: row.name,
                einfahrzeit_seconds: row.einfahrzeit_seconds,
                act_duration_seconds: row.act_duration_seconds,
                judge_duration_seconds: row.judge_duration_seconds,
            },
        )
    })
    .collect();
    let acts_in_order: Vec<ActSlot> = sqlx::query!(
        r#"
        SELECT
            id as "id!: ActId",
            name,
            category_id as "category_id!: CategoryId",
            category as "category!",
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime",
            music_started_at as "music_started_at: OffsetDateTime"
        FROM view_act
        WHERE category_id IS NOT NULL AND withdrawn_at IS NULL
        ORDER BY "order" NULLS FIRST
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| ActSlot {
        id: row.id,
        name: row.name,
        category_id: row.category_id,
        category: row.category,
        started_at: row.started_at,
        ended_at: row.ended_at,
        music_started_at: row.music_started_at,
        stored: false,
    })
    .collect();
    let acts: HashMap<ActId, ActSlot> = acts_in_order
        .iter()
        .map(|act| (act.id, act.clone()))
        .collect();

    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!",
            kind as "kind: EntryKind",
            earliest_start_time as "earliest_start_time: OffsetDateTime",
            duration_seconds,
            CASE WHEN internal AND ?1 THEN ?2 ELSE label END as "label: String",
            category_id as "category_id: CategoryId",
            (SELECT name FROM categories WHERE categories.id = timeplan.category_id) as category,
            final_id as "final_id: FinalId",
            (SELECT name FROM finals WHERE finals.id = timeplan.final_id) as final_name,
            einfahrzeit_seconds,
            show_photos,
            internal AND NOT ?1 as "internal!: bool",
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime"
        FROM timeplan
        ORDER BY position, id
        "#,
        public,
        INTERNAL_PUBLIC_LABEL,
    )
    .fetch_all(&mut *db)
    .await?;

    // Skipped heats whose final is planned: their acts start directly in
    // it, by category. A heat fills at most one final; should old data have
    // more, the first in final order counts.
    let planned_finals: HashSet<FinalId> = rows
        .iter()
        .filter(|row| row.kind == EntryKind::Acts)
        .filter_map(|row| row.final_id)
        .collect();
    let mut direct_final: HashMap<CategoryId, FinalId> = HashMap::new();
    let mut skipped_heats: HashSet<HeatId> = HashSet::new();
    for row in sqlx::query!(
        r#"
        SELECT
            heats.id as "heat_id!: HeatId",
            categories.id as "category_id?: CategoryId",
            (
                SELECT final_slots.final_id
                FROM final_slots JOIN finals ON finals.id = final_slots.final_id
                WHERE final_slots.heat_id = heats.id
                ORDER BY finals."order", finals.name
                LIMIT 1
            ) as "final_id?: FinalId"
        FROM heats LEFT JOIN categories ON categories.heat_id = heats.id
        WHERE heats.skip
        "#
    )
    .fetch_all(&mut *db)
    .await?
    {
        let Some(final_id) = row.final_id.filter(|id| planned_finals.contains(id)) else {
            continue;
        };
        skipped_heats.insert(row.heat_id);
        if let Some(category_id) = row.category_id {
            direct_final.insert(category_id, final_id);
        }
    }
    let direct_acts: HashMap<ActId, FinalId> = acts_in_order
        .iter()
        .filter_map(|act| Some((act.id, *direct_final.get(&act.category_id)?)))
        .collect();

    // The finalist slots of each final, without those of skipped heats.
    let mut finalists: HashMap<FinalId, Vec<FinalistSlot>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            final_slots.id as "id!: FinalSlotId",
            final_slots.final_id as "final_id!: FinalId",
            finals.name as "final_name!",
            final_slots.heat_id as "heat_id!: HeatId",
            heats.code as "heat!",
            number,
            act_id as "act_id: ActId",
            finals.placement_published,
            final_slots.started_at as "started_at: OffsetDateTime",
            final_slots.ended_at as "ended_at: OffsetDateTime",
            final_slots.music_started_at as "music_started_at: OffsetDateTime"
        FROM final_slots
        JOIN finals ON finals.id = final_slots.final_id
        JOIN heats ON heats.id = final_slots.heat_id
        ORDER BY finals.name, position
        "#
    )
    .fetch_all(&mut *db)
    .await?
    {
        // A withdrawn act has no category at the moment: the slot shows as
        // open.
        if skipped_heats.contains(&row.heat_id) {
            continue;
        }
        let act = row
            .act_id
            .filter(|_| row.placement_published)
            .and_then(|id| acts.get(&id))
            .cloned();
        finalists
            .entry(row.final_id)
            .or_default()
            .push(FinalistSlot {
                id: row.id,
                final_id: row.final_id,
                final_name: row.final_name,
                heat_id: row.heat_id,
                group_label: row.heat,
                number: row.number,
                act,
                started_at: row.started_at,
                ended_at: row.ended_at,
                music_started_at: row.music_started_at,
            });
    }

    let mut stored: HashMap<i64, Vec<Slot>> = HashMap::new();
    // Acts in a block's slots, also those hidden at the moment: they never
    // show up in their home block on top.
    let mut placed: HashSet<ActId> = HashSet::new();
    // The direct acts and finalist slots a final's block stores.
    let mut placed_in_final: HashSet<FinalSlotId> = HashSet::new();
    let mut placed_direct: HashSet<ActId> = HashSet::new();
    // The start blocks, with the final running in them.
    let act_blocks: HashMap<i64, Option<FinalId>> = rows
        .iter()
        .filter(|row| row.kind == EntryKind::Acts)
        .map(|row| (row.id, row.final_id))
        .collect();
    for row in sqlx::query!(
        r#"
        SELECT
            id as "id!",
            timeplan_id,
            act_id as "act_id: ActId",
            label,
            duration_seconds,
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime",
            heat_id as "heat_id: HeatId",
            number
        FROM timeplan_slots
        ORDER BY timeplan_id, position, id
        "#
    )
    .fetch_all(&mut *db)
    .await?
    {
        let Some(&final_id) = act_blocks.get(&row.timeplan_id) else {
            continue;
        };
        let slots = stored.entry(row.timeplan_id).or_default();
        match (row.act_id, row.heat_id, row.number) {
            // In a final's block: only its direct acts. Elsewhere: no
            // direct act, it runs in its final.
            (Some(act_id), _, _) => match (final_id, direct_acts.get(&act_id)) {
                (Some(id), Some(direct)) if id == *direct => {
                    placed_direct.insert(act_id);
                    slots.push(Slot::Act(ActSlot {
                        stored: true,
                        ..acts[&act_id].clone()
                    }));
                }
                (None, None) => {
                    placed.insert(act_id);
                    if let Some(act) = acts.get(&act_id) {
                        slots.push(Slot::Act(ActSlot {
                            stored: true,
                            ..act.clone()
                        }));
                    }
                }
                _ => {}
            },
            // A final's slot, by heat and number; gone or unused ones are
            // left out.
            (None, Some(heat_id), Some(number)) => {
                let finalist = final_id
                    .and_then(|id| finalists.get(&id))
                    .and_then(|slots| {
                        slots
                            .iter()
                            .find(|slot| slot.heat_id == heat_id && slot.number == number)
                    })
                    .filter(|slot| placed_in_final.insert(slot.id));
                if let Some(finalist) = finalist {
                    slots.push(Slot::Finalist(finalist.clone()));
                }
            }
            _ => slots.push(Slot::Break(BreakSlot {
                id: row.id,
                label: row.label.unwrap_or_default(),
                duration_seconds: row.duration_seconds.unwrap_or(0),
                started_at: row.started_at,
                ended_at: row.ended_at,
            })),
        }
    }

    let homes = home_blocks(rows.iter().filter_map(|row| {
        (row.kind == EntryKind::Acts).then_some((
            row.id,
            row.category_id?,
            row.started_at.is_some(),
        ))
    }));
    let mut entries: Vec<Entry> = Vec::with_capacity(rows.len());
    for row in &rows {
        let manually_ordered = stored.contains_key(&row.id);
        let mut slots = stored.remove(&row.id).unwrap_or_default();
        let mut einfahrzeit_seconds = 0;
        let mut preliminary_skipped = false;
        if row.kind == EntryKind::Acts {
            // A final's block holds its stored slots, then its other
            // finalist slots, then the other acts of its skipped heats, by
            // category.
            if let Some(final_id) = row.final_id {
                slots.extend(
                    finalists
                        .remove(&final_id)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|slot| !placed_in_final.contains(&slot.id))
                        .map(Slot::Finalist),
                );
                let mut direct: Vec<&ActSlot> = acts_in_order
                    .iter()
                    .filter(|act| {
                        direct_acts.get(&act.id) == Some(&final_id)
                            && !placed_direct.contains(&act.id)
                    })
                    .collect();
                direct.sort_by_key(|act| category_order.get(&act.category_id));
                slots.extend(direct.into_iter().cloned().map(Slot::Act));
                einfahrzeit_seconds = finals.get(&final_id).map_or(0, |f| f.einfahrzeit_seconds);
            }
            if let Some(home) = row.category_id {
                preliminary_skipped = direct_final.contains_key(&home);
                if homes.get(&home) == Some(&row.id) {
                    slots.extend(
                        acts_in_order
                            .iter()
                            .filter(|act| {
                                act.category_id == home
                                    && !placed.contains(&act.id)
                                    && !direct_acts.contains_key(&act.id)
                            })
                            .cloned()
                            .map(Slot::Act),
                    );
                }
                einfahrzeit_seconds = categories.get(&home).map_or(0, |c| c.einfahrzeit_seconds);
            }
            einfahrzeit_seconds = row.einfahrzeit_seconds.unwrap_or(einfahrzeit_seconds);
            // Stable: started slots first, then the others, each in their
            // planned order. Not by start time: that would trust the clock
            // (and imported data) more than the plan.
            slots.sort_by_key(|slot| !slot.is_started());
        }
        entries.push(Entry {
            id: row.id,
            kind: row.kind,
            earliest_start_time: row.earliest_start_time,
            duration_seconds: row.duration_seconds,
            label: row.label.clone(),
            category_id: row.category_id,
            category: row.category.clone(),
            final_id: row.final_id,
            final_name: row.final_name.clone(),
            einfahrzeit_override: row.einfahrzeit_seconds,
            show_photos: row.show_photos,
            internal: row.internal,
            started_at: row.started_at,
            ended_at: row.ended_at,
            slots,
            manually_ordered,
            preliminary_skipped,
            einfahrzeit_seconds,
            warmup_seconds: 0,
        });
    }

    let blocks: Vec<Block> = entries
        .iter()
        .map(|entry| match entry.kind {
            // A block without a category (e.g. "Nachstarter") warms up only
            // if it has an own Einfahrzeit; a shared warm-up waiting for the
            // next category passes it by.
            EntryKind::Acts
                if entry.category_id.is_none()
                    && entry.final_id.is_none()
                    && entry.einfahrzeit_override.is_none() =>
            {
                Block::Acts {
                    einfahrzeit_seconds: 0,
                    empty: true,
                }
            }
            EntryKind::Acts => Block::Acts {
                einfahrzeit_seconds: entry.einfahrzeit_seconds,
                empty: entry.is_empty(),
            },
            EntryKind::Custom => Block::Custom,
        })
        .collect();
    for (entry, warmup) in entries.iter_mut().zip(effective_warmups(&blocks)) {
        entry.warmup_seconds = warmup;
    }

    let block_of_act = entries
        .iter()
        .flat_map(|entry| {
            entry
                .slots
                .iter()
                .filter_map(move |slot| slot.act().map(|act| (act.id, entry.id)))
        })
        .collect();

    Ok(Timeplan {
        entries,
        categories,
        finals,
        acts,
        block_of_act,
        direct_acts,
    })
}

/// The ids of the timeplan entries in order.
pub(crate) async fn entry_ids(db: &mut SqliteConnection) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(r#"SELECT id as "id!" FROM timeplan ORDER BY position, id"#)
        .fetch_all(db)
        .await
}

/// Numbers the entries `1..` in the given order.
pub(crate) async fn set_entry_order(db: &mut SqliteConnection, ids: &[i64]) -> sqlx::Result<()> {
    for (position, id) in (1_i64..).zip(ids) {
        sqlx::query!(
            "UPDATE timeplan SET position = $1 WHERE id = $2",
            position,
            id
        )
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Swaps an entry with the one before it (`up`) or after it. Returns
/// whether there was one.
pub(crate) async fn swap_with_neighbour(
    db: &mut SqliteConnection,
    id: i64,
    up: bool,
) -> sqlx::Result<bool> {
    let mut ids = entry_ids(&mut *db).await?;
    let Some(index) = ids.iter().position(|&e| e == id) else {
        return Ok(false);
    };
    let other = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1).filter(|&i| i < ids.len())
    };
    let Some(other) = other else {
        return Ok(false);
    };
    ids.swap(index, other);
    set_entry_order(db, &ids).await?;
    Ok(true)
}

/// The database's current time, like `datetime('now')` but with
/// milliseconds. One step of the live timeplan writes the same time
/// everywhere, so "ended in the same step" can be told from the times; two
/// steps (separate write transactions) don't share a millisecond.
pub(crate) async fn now(db: &mut SqliteConnection) -> sqlx::Result<String> {
    sqlx::query_scalar!(r#"SELECT strftime('%Y-%m-%d %H:%M:%f', 'now') as "now!: String""#)
        .fetch_one(db)
        .await
}

/// Starts a slot at `now` (see [`now`]).
pub(crate) async fn start_slot(
    db: &mut SqliteConnection,
    slot: &Slot,
    now: &str,
) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET started_at = $1 WHERE id = $2", now, act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET started_at = $1 WHERE id = $2",
                now,
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET started_at = $1 WHERE id = $2",
                now,
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Ends a running slot at `now` (see [`now`]).
pub(crate) async fn end_slot(
    db: &mut SqliteConnection,
    slot: &Slot,
    now: &str,
) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET ended_at = $1 WHERE id = $2", now, act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET ended_at = $1 WHERE id = $2",
                now,
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET ended_at = $1 WHERE id = $2",
                now,
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Puts a running slot back to planned. An act (or finalist) forgets its
/// music start.
pub(crate) async fn unstart_slot(db: &mut SqliteConnection, slot: &Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!(
                "UPDATE acts SET started_at = NULL, music_started_at = NULL WHERE id = $1",
                act.id
            )
            .execute(db)
            .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET started_at = NULL WHERE id = $1",
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET started_at = NULL, music_started_at = NULL WHERE id = $1",
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Makes an ended slot run again.
pub(crate) async fn reopen_slot(db: &mut SqliteConnection, slot: &Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET ended_at = NULL WHERE id = $1", act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET ended_at = NULL WHERE id = $1",
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET ended_at = NULL WHERE id = $1",
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}
