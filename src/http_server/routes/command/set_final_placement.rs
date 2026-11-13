use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetFinalPlacementResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FinalSlotPlacement {
    slot_id: Uuid,
    /// The act placed in the slot; `null` empties it.
    act_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFinalPlacementBody {
    /// The final.
    id: Uuid,
    /// The slots to change; the others keep their acts.
    slots: Vec<FinalSlotPlacement>,
}

/// Place acts in the slots of a final.
///
/// A slot takes the acts of its heat's categories, each at most once per
/// final.
/// A slot that has started keeps its act. Once the placement is published,
/// changes show in the timeplan right away.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/set_final_placement",
    request_body=SetFinalPlacementBody,
    responses(
        (status=200, content_type="application/json", body=SetFinalPlacementResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn set_final_placement(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ManageResults>,
    Json(body): Json<SetFinalPlacementBody>,
) -> Result<Json<SetFinalPlacementResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let Some(final_name) = crate::finals::name_of(&mut tx, body.id).await? else {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    };

    let slots = sqlx::query!(
        r#"
        SELECT
            final_slots.id as "id!: Uuid",
            final_slots.heat_id as "heat_id!: Uuid",
            heats.name as "heat!",
            number,
            act_id as "act_id: Uuid",
            final_slots.started_at IS NOT NULL as "started!: bool"
        FROM final_slots
        JOIN heats ON heats.id = final_slots.heat_id
        WHERE final_id = ?
        "#,
        body.id
    )
    .fetch_all(&mut *tx)
    .await?;
    // The heat of each category.
    let heats: HashMap<String, Uuid> = sqlx::query!(
        r#"SELECT name as "name!", heat_id as "heat_id!: Uuid" FROM categories WHERE heat_id IS NOT NULL"#
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| (row.name, row.heat_id))
    .collect();

    // The placement after the change, to check it as a whole.
    let mut placement: HashMap<Uuid, Option<Uuid>> =
        slots.iter().map(|slot| (slot.id, slot.act_id)).collect();
    let mut changed = vec![];
    for change in &body.slots {
        let Some(slot) = slots.iter().find(|slot| slot.id == change.slot_id) else {
            return Err(HttpError::ErrorMessages(
                "Diesen Finalplatz gibt es in diesem Finale nicht.".to_string(),
            ));
        };
        if slot.act_id == change.act_id {
            continue;
        }
        let label = crate::finals::finalist_label(slot.number, &slot.heat);
        if slot.started {
            return Err(HttpError::ErrorMessages(format!(
                "„{label}“ hat schon begonnen, die Kür bleibt."
            )));
        }
        if let Some(act_id) = change.act_id {
            // Withdrawn acts don't start: they count as without category.
            let category = sqlx::query_scalar!(
                r#"SELECT CAST(CASE WHEN withdrawn_at IS NULL THEN category END AS TEXT) as "category: String"
                FROM view_act WHERE id = ?"#,
                act_id
            )
            .fetch_optional(&mut *tx)
            .await?;
            match category {
                None => {
                    return Err(HttpError::ErrorMessages(format!(
                        "Die Kür für „{label}“ gibt es nicht."
                    )));
                }
                Some(None) => {
                    return Err(HttpError::ErrorMessages(format!(
                        "Die Kür für „{label}“ ist zurückgezogen oder unvollständig."
                    )));
                }
                Some(Some(category)) if heats.get(&category) != Some(&slot.heat_id) => {
                    return Err(HttpError::ErrorMessages(format!(
                        "Die Kür für „{label}“ startet in {category}, einer Kategorie, die nicht zur Vorlaufklasse „{}“ gehört.",
                        slot.heat
                    )));
                }
                Some(Some(_)) => {}
            }
        }
        placement.insert(slot.id, change.act_id);
        changed.push((slot.id, change.act_id));
    }
    let mut seen = HashSet::new();
    if placement
        .values()
        .flatten()
        .any(|act_id| !seen.insert(*act_id))
    {
        return Err(HttpError::ErrorMessages(
            "Eine Kür kann nur einen Finalplatz eines Finales bekommen.".to_string(),
        ));
    }

    info!(
        "Placing acts in {} slots of final {:?}",
        changed.len(),
        final_name
    );
    // Empty the changed slots first, so acts can swap slots without
    // clashing in the unique index.
    for (slot_id, _) in &changed {
        sqlx::query!("UPDATE final_slots SET act_id = NULL WHERE id = ?", slot_id)
            .execute(&mut *tx)
            .await?;
    }
    for (slot_id, act_id) in &changed {
        sqlx::query!(
            "UPDATE final_slots SET act_id = ? WHERE id = ?",
            act_id,
            slot_id
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    // A published final's songs coming up may have changed.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetFinalPlacementResponse {}))
}
