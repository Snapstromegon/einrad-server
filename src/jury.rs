//! The jury registration (#225): clubs register their judges (JurorInnen)
//! for the heats' Vorlaufs and the finals (jury units), admins check each club's jury
//! obligation and plan the jury. Everything is behind the
//! `jury_registration` feature of the competition (see `Features`).
//!
//! Types and rules live in `db_core::jury`; the SQL in the backend.

use axum::{extract::FromRequestParts, http::request::Parts};
use db_core::{DbHandle, ids::FinalId};
use time::Date;

pub use db_core::jury::*;

use crate::{
    competition::Features,
    http_server::{
        HttpError,
        routes::query::predict_timeplan::{self, TimeplanEntry},
    },
    timeplan::Audience,
};

/// Guard for every jury endpoint: they don't exist (404) while the feature
/// is off. Read per request, so turning it on or off applies immediately.
#[derive(Debug)]
pub struct JuryEnabled;

impl<S: Send + Sync> FromRequestParts<S> for JuryEnabled {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(repo) = parts.extensions.get::<DbHandle>() else {
            return Err(HttpError::NotFound);
        };
        if Features::from_repo(repo.as_ref()).await?.jury_registration {
            Ok(JuryEnabled)
        } else {
            Err(HttpError::NotFound)
        }
    }
}

/// Loads the jury units in the order of the timeplan: by the first block of
/// the heat's categories or of the final; units the timeplan doesn't run
/// last, by name. The day is the planned day of that block.
pub async fn list_units(repo: &dyn db_core::Db) -> Result<Vec<JuryUnit>, HttpError> {
    let JuryUnitRows {
        units: rows,
        categories,
        feeds,
        final_types,
    } = repo.jury_unit_rows().await?;

    // Where each unit first runs: the position and planned day of its block.
    // Without a timeplan (or without a start time for its first block) there
    // are no days.
    let blocks = match predict_timeplan::predict(repo, Audience::OrganizerTeam).await {
        Ok(plan) => plan.items,
        Err(e) => {
            tracing::warn!("No days for the jury units, the timeplan can't be predicted: {e:?}");
            vec![]
        }
    };
    let first_block = |matches: &dyn Fn(&TimeplanEntry) -> bool| {
        blocks
            .iter()
            .enumerate()
            .find(|(_, item)| matches(&item.timeplan_entry))
            .map(|(position, item)| (position, item.planned_start.date()))
    };
    let mut units: Vec<(Option<(usize, Date)>, JuryUnit)> = rows
        .into_iter()
        .map(|row| {
            let block = match row.kind {
                JuryUnitKind::Heat => first_block(&|entry| match entry {
                    TimeplanEntry::Acts {
                        home_category: Some(category),
                        ..
                    } => categories
                        .iter()
                        .any(|c| &c.name == category && c.heat_id == Some(row.id.as_heat())),
                    _ => false,
                }),
                JuryUnitKind::Final => first_block(&|entry| match entry {
                    TimeplanEntry::Acts {
                        final_id: Some(id), ..
                    } => *id == row.id.as_final(),
                    _ => false,
                }),
            };
            let final_id = match row.kind {
                JuryUnitKind::Heat => feeds
                    .iter()
                    .find(|f| f.heat_id == row.id.as_heat())
                    .map(|f| f.final_id),
                JuryUnitKind::Final => None,
            };
            let own_type =
                |id: FinalId| final_types.iter().find(|f| f.id == id).map(|f| f.act_type);
            let act_type = match row.kind {
                JuryUnitKind::Heat => categories
                    .iter()
                    .find(|c| c.heat_id == Some(row.id.as_heat()))
                    .map(|c| c.act_type)
                    .or_else(|| final_id.and_then(own_type)),
                JuryUnitKind::Final => own_type(row.id.as_final()),
            };
            let unit = JuryUnit {
                act_type,
                final_id,
                id: row.id,
                kind: row.kind,
                name: row.name,
                code: row.code,
                description: row.description.filter(|d| !d.trim().is_empty()),
                registrable: row.registrable,
                registrable_setting: row.registrable_setting,
                skipped: row.skipped,
                deployments: row.deployments,
                needed_technical: row.needed_technical,
                needed_performance: row.needed_performance,
                needed_dismounts: row.needed_dismounts,
                day: block.map(|(_, day)| day),
            };
            (block, unit)
        })
        .collect();
    units.sort_by(|(a_block, a), (b_block, b)| {
        let key = |block: &Option<(usize, Date)>, unit: &JuryUnit| {
            (
                block.map_or(usize::MAX, |(position, _)| position),
                unit.kind == JuryUnitKind::Final,
                unit.name.clone(),
            )
        };
        key(a_block, a).cmp(&key(b_block, b))
    });
    Ok(units.into_iter().map(|(_, unit)| unit).collect())
}
