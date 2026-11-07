use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    timeplan::{self, Audience, EntryKind, Slot},
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanAct {
    pub status: TimeplanItemStatus,
    pub id: Uuid,
    pub name: String,
    /// The act's own category; in a mixed block it differs from the block's.
    pub category: String,
    pub category_description: String,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<time::OffsetDateTime>,
    /// When the act's music first started while it ran; the act timer counts
    /// from here.
    #[serde(with = "time::serde::iso8601::option")]
    pub music_started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_end: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_end: time::OffsetDateTime,
}

/// A break inside a start block, e.g. an extra warm-up. It runs until
/// "Weiter" ends it; `duration_seconds` is only its plan.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanBreak {
    pub status: TimeplanItemStatus,
    pub id: i64,
    pub label: String,
    pub duration_seconds: i64,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_end: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_end: time::OffsetDateTime,
}

/// A reserved slot of a final, e.g. "3. Finalist (U17–U21)": it runs and is
/// judged like an act. Planned before the finalists are known; once the
/// final's placement is published, `act` is the act placed in it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanFinalist {
    pub status: TimeplanItemStatus,
    pub id: Uuid,
    /// "3. Finalist (U17–U21)".
    pub label: String,
    pub final_name: String,
    pub final_description: String,
    /// Where the finalist comes from, e.g. "U17–U21"; may be empty.
    pub group_label: String,
    /// Counts within the group.
    pub number: i64,
    /// The act placed in the slot, once the placement is published.
    pub act: Option<TimeplanFinalistAct>,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<time::OffsetDateTime>,
    /// When the placed act's music first started while the slot ran; the
    /// timer counts from here.
    #[serde(with = "time::serde::iso8601::option")]
    pub music_started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_end: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_end: time::OffsetDateTime,
}

/// The act placed in a final's slot. Its song plays in the slot.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanFinalistAct {
    pub id: Uuid,
    pub name: String,
    /// The act's category, from its heat.
    pub category: String,
    pub category_description: String,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub enum TimeplanSlot {
    Act(TimeplanAct),
    Break(TimeplanBreak),
    Finalist(TimeplanFinalist),
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub enum TimeplanEntry {
    /// A start block: acts (of any category) and breaks, or a final's
    /// reserved slots.
    Acts {
        /// The block's own name, e.g. "Nachstarter"; usually none.
        label: Option<String>,
        /// What to show: the label, otherwise the home category's (or
        /// final's) description.
        description: String,
        /// The category whose acts run here by default.
        home_category: Option<String>,
        /// The final running here instead of a category; its slots are
        /// `Finalist` slots.
        final_name: Option<String>,
        /// The categories of the block's acts, in order of appearance.
        categories: Vec<String>,
        duration_seconds: i64,
        /// The configured warm-up: the block's own or the home category's.
        einfahrzeit_seconds: i64,
        /// The warm-up this block really gets: none for an empty block, and a
        /// block sharing the warm-up of an empty category before it gets
        /// that one (see `crate::timeplan::effective_warmups`).
        warmup_seconds: i64,
        /// The order was edited by hand (`set_block_order`).
        manually_ordered: bool,
        slots: Vec<TimeplanSlot>,
    },
    Custom {
        label: String,
        duration_seconds: i64,
        /// Photos next to the clock on the info board.
        show_photos: bool,
        /// Only the organizer team sees it (Interner Zeitplaneintrag).
        internal: bool,
    },
}

impl TimeplanEntry {
    pub fn duration(&self) -> time::Duration {
        match self {
            TimeplanEntry::Acts {
                duration_seconds, ..
            } => time::Duration::seconds(*duration_seconds),
            TimeplanEntry::Custom {
                duration_seconds, ..
            } => time::Duration::seconds(*duration_seconds),
        }
    }

    /// The acts whose songs play in a start block, in order: its acts and
    /// the acts placed in its finalist slots, each with its slot's status.
    pub fn songs(&self) -> impl Iterator<Item = (Uuid, &TimeplanItemStatus)> {
        let slots = match self {
            TimeplanEntry::Acts { slots, .. } => slots.as_slice(),
            TimeplanEntry::Custom { .. } => &[],
        };
        slots.iter().filter_map(|slot| match slot {
            TimeplanSlot::Act(act) => Some((act.id, &act.status)),
            TimeplanSlot::Finalist(finalist) => Some((finalist.act.as_ref()?.id, &finalist.status)),
            TimeplanSlot::Break(_) => None,
        })
    }
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema, Eq, PartialEq)]
pub enum TimeplanItemStatus {
    Planned,
    Started,
    Ended,
}

impl TimeplanItemStatus {
    fn of(
        started_at: Option<time::OffsetDateTime>,
        ended_at: Option<time::OffsetDateTime>,
    ) -> Result<Self, HttpError> {
        match (started_at, ended_at) {
            (Some(_), Some(_)) => Ok(Self::Ended),
            (Some(_), None) => Ok(Self::Started),
            (None, None) => Ok(Self::Planned),
            (None, Some(_)) => Err(HttpError::ErrorMessages(
                "Ungültiger Zeitplaneintrag".to_string(),
            )),
        }
    }
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanItem {
    /// The timeplan entry's id.
    pub id: i64,
    pub status: TimeplanItemStatus,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub predicted_end: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_start: time::OffsetDateTime,
    #[serde(with = "time::serde::iso8601")]
    pub planned_end: time::OffsetDateTime,
    pub planned_duration: i64,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<time::OffsetDateTime>,
    pub timeplan_entry: TimeplanEntry,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Timeplan {
    pub offset: i64,
    pub items: Vec<TimeplanItem>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct PredictTimeplanQuery {
    /// Leave out the internal entries even for the organizer team, e.g. for
    /// the info board.
    #[serde(default)]
    public: bool,
}

/// Predicts when each timeplan entry, act and break starts and ends.
#[utoipa::path(
    get,
    tags=["query", "timeplan"],
    path="/predict_timeplan",
    params(PredictTimeplanQuery),
    responses(
        (status=200, content_type="application/json", body=Timeplan),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn predict_timeplan(
    Extension(db): Extension<ReloadableSqlite>,
    internal: Option<Granted<permit::ViewInternalEntries>>,
    Query(query): Query<PredictTimeplanQuery>,
) -> Result<Json<Timeplan>, HttpError> {
    let db = db.get().await.clone();
    let audience = if internal.is_some() && !query.public {
        Audience::OrganizerTeam
    } else {
        Audience::Public
    };
    Ok(Json(predict(&db, audience).await?))
}

/// The prediction behind `predict_timeplan`, also used by the startlist CSV,
/// the player hub and the metrics.
pub async fn predict(db: &sqlx::SqlitePool, audience: Audience) -> Result<Timeplan, HttpError> {
    // One read transaction, so the blocks and acts are read consistently.
    let mut tx = db.begin().await?;
    let loaded = timeplan::load_for(&mut tx, audience).await?;
    tx.commit().await?;
    predict_from(loaded, time::OffsetDateTime::now_utc())
}

/// The prediction for a loaded timeplan at `now`.
pub fn predict_from(
    loaded: timeplan::Timeplan,
    now: time::OffsetDateTime,
) -> Result<Timeplan, HttpError> {
    let Some(first_entry) = loaded.entries.first() else {
        return Ok(Timeplan {
            offset: 0,
            items: vec![],
        });
    };
    let mut next_planned_start_time =
        first_entry
            .earliest_start_time
            .ok_or(HttpError::ErrorMessages(
                "Der erste Zeitplaneintrag braucht eine früheste Startzeit.".to_string(),
            ))?;

    let mut next_predicted_start_time = now.max(next_planned_start_time);

    let mut timeplan = Timeplan {
        offset: 0,
        items: vec![],
    };

    // The offset is the predicted delay of the next step that hasn't started
    // (an act, a break, or an entry's start). Once everything has started,
    // it's the delay of the last start. It doesn't jump between acts or at
    // the end.
    let mut next_offset = None;
    let mut last_offset = None;

    for entry in loaded.entries {
        if let Some(earliest_start_time) = entry.earliest_start_time {
            next_planned_start_time = earliest_start_time.max(next_planned_start_time);
            next_predicted_start_time = entry
                .started_at
                .unwrap_or(earliest_start_time.max(next_predicted_start_time));
        }

        let item_planned_start_time = next_planned_start_time;
        let item_predicted_start_time = next_predicted_start_time;
        if let Some(started_at) = entry.started_at {
            last_offset = Some((started_at - item_planned_start_time).whole_seconds());
        } else if next_offset.is_none() {
            next_offset =
                Some((item_predicted_start_time - item_planned_start_time).whole_seconds());
        }

        let timeplan_entry = match entry.kind {
            EntryKind::Acts => {
                let empty = entry.is_empty();
                let block_start = next_planned_start_time;
                next_predicted_start_time += time::Duration::seconds(entry.warmup_seconds);
                next_planned_start_time += time::Duration::seconds(entry.warmup_seconds);

                let mut categories: Vec<String> = vec![];
                let mut slots = vec![];
                for slot in &entry.slots {
                    // An empty block is skipped, its breaks too: they take no
                    // time.
                    if empty {
                        if let Slot::Break(pause) = slot {
                            slots.push(TimeplanSlot::Break(TimeplanBreak {
                                status: TimeplanItemStatus::of(pause.started_at, pause.ended_at)?,
                                id: pause.id,
                                label: pause.label.clone(),
                                duration_seconds: pause.duration_seconds,
                                started_at: pause.started_at,
                                ended_at: pause.ended_at,
                                predicted_start: item_predicted_start_time,
                                predicted_end: item_predicted_start_time,
                                planned_start: block_start,
                                planned_end: block_start,
                            }));
                        }
                        continue;
                    }

                    next_predicted_start_time = now.max(next_predicted_start_time);
                    if let Some(started_at) = slot.started_at() {
                        next_predicted_start_time = started_at;
                        last_offset = Some((started_at - next_planned_start_time).whole_seconds());
                    } else if next_offset.is_none() {
                        next_offset = Some(
                            (next_predicted_start_time - next_planned_start_time).whole_seconds(),
                        );
                    }

                    match slot {
                        Slot::Act(act) => {
                            let category = loaded.categories.get(&act.category);
                            let act_duration = time::Duration::seconds(
                                category.map_or(0, |c| c.act_duration_seconds),
                            );
                            let judge_duration = time::Duration::seconds(
                                category.map_or(0, |c| c.judge_duration_seconds),
                            );
                            if !categories.contains(&act.category) {
                                categories.push(act.category.clone());
                            }
                            slots.push(TimeplanSlot::Act(TimeplanAct {
                                status: TimeplanItemStatus::of(act.started_at, act.ended_at)?,
                                id: act.id,
                                name: act.name.clone(),
                                category: act.category.clone(),
                                category_description: category.map_or_else(
                                    || act.category.clone(),
                                    |c| c.description.clone(),
                                ),
                                act_duration_seconds: act_duration.whole_seconds(),
                                judge_duration_seconds: judge_duration.whole_seconds(),
                                started_at: act.started_at,
                                ended_at: act.ended_at,
                                music_started_at: act.music_started_at,
                                predicted_start: next_predicted_start_time,
                                predicted_end: act
                                    .ended_at
                                    .unwrap_or(next_predicted_start_time + act_duration),
                                planned_start: next_planned_start_time,
                                planned_end: next_planned_start_time + act_duration,
                            }));
                            // The next act starts after the judging.
                            next_predicted_start_time += act_duration + judge_duration;
                            next_planned_start_time += act_duration + judge_duration;
                        }
                        Slot::Break(pause) => {
                            let duration = time::Duration::seconds(pause.duration_seconds);
                            slots.push(TimeplanSlot::Break(TimeplanBreak {
                                status: TimeplanItemStatus::of(pause.started_at, pause.ended_at)?,
                                id: pause.id,
                                label: pause.label.clone(),
                                duration_seconds: pause.duration_seconds,
                                started_at: pause.started_at,
                                ended_at: pause.ended_at,
                                predicted_start: next_predicted_start_time,
                                predicted_end: pause
                                    .ended_at
                                    .unwrap_or(next_predicted_start_time + duration),
                                planned_start: next_planned_start_time,
                                planned_end: next_planned_start_time + duration,
                            }));
                            // Ending a break starts the next slot right away.
                            next_predicted_start_time = match pause.ended_at {
                                Some(ended_at) => ended_at,
                                None => next_predicted_start_time + duration,
                            };
                            next_planned_start_time += duration;
                        }
                        Slot::Finalist(finalist) => {
                            let info = loaded.finals.get(&finalist.final_name);
                            let act_duration =
                                time::Duration::seconds(info.map_or(0, |f| f.act_duration_seconds));
                            let judge_duration = time::Duration::seconds(
                                info.map_or(0, |f| f.judge_duration_seconds),
                            );
                            slots.push(TimeplanSlot::Finalist(TimeplanFinalist {
                                status: TimeplanItemStatus::of(
                                    finalist.started_at,
                                    finalist.ended_at,
                                )?,
                                id: finalist.id,
                                label: finalist.label(),
                                final_name: finalist.final_name.clone(),
                                final_description: info.map_or_else(
                                    || finalist.final_name.clone(),
                                    |f| f.description.clone(),
                                ),
                                group_label: finalist.group_label.clone(),
                                number: finalist.number,
                                act: finalist.act.as_ref().map(|act| TimeplanFinalistAct {
                                    id: act.id,
                                    name: act.name.clone(),
                                    category: act.category.clone(),
                                    category_description: loaded
                                        .categories
                                        .get(&act.category)
                                        .map_or_else(
                                            || act.category.clone(),
                                            |c| c.description.clone(),
                                        ),
                                }),
                                act_duration_seconds: act_duration.whole_seconds(),
                                judge_duration_seconds: judge_duration.whole_seconds(),
                                started_at: finalist.started_at,
                                ended_at: finalist.ended_at,
                                music_started_at: finalist.music_started_at,
                                predicted_start: next_predicted_start_time,
                                predicted_end: finalist
                                    .ended_at
                                    .unwrap_or(next_predicted_start_time + act_duration),
                                planned_start: next_planned_start_time,
                                planned_end: next_planned_start_time + act_duration,
                            }));
                            // Like an act: the next slot starts after the
                            // judging.
                            next_predicted_start_time += act_duration + judge_duration;
                            next_planned_start_time += act_duration + judge_duration;
                        }
                    }
                }

                let home = entry
                    .category
                    .as_ref()
                    .and_then(|name| loaded.categories.get(name))
                    .or_else(|| {
                        entry
                            .final_name
                            .as_ref()
                            .and_then(|name| loaded.finals.get(name))
                    });
                let description = entry
                    .label
                    .clone()
                    .filter(|label| !label.trim().is_empty())
                    .or_else(|| home.map(|c| c.description.clone()))
                    .or_else(|| entry.category.clone())
                    .or_else(|| entry.final_name.clone())
                    .unwrap_or_else(|| "Startblock".to_string());
                TimeplanEntry::Acts {
                    label: entry.label,
                    description,
                    home_category: entry.category,
                    final_name: entry.final_name,
                    categories,
                    duration_seconds: (next_planned_start_time - block_start).whole_seconds(),
                    einfahrzeit_seconds: entry.einfahrzeit_seconds,
                    warmup_seconds: entry.warmup_seconds,
                    manually_ordered: entry.manually_ordered,
                    slots,
                }
            }
            EntryKind::Custom => match (entry.duration_seconds, entry.label) {
                (Some(duration), Some(label)) => {
                    next_predicted_start_time += time::Duration::seconds(duration);
                    next_planned_start_time += time::Duration::seconds(duration);
                    TimeplanEntry::Custom {
                        label,
                        duration_seconds: duration,
                        show_photos: entry.show_photos,
                        internal: entry.internal,
                    }
                }
                _ => {
                    return Err(HttpError::ErrorMessages(
                        "Ungültiger Zeitplaneintrag".to_string(),
                    ));
                }
            },
        };

        let item = TimeplanItem {
            id: entry.id,
            status: TimeplanItemStatus::of(entry.started_at, entry.ended_at)?,
            predicted_start: entry.started_at.unwrap_or(item_predicted_start_time),
            predicted_end: entry
                .ended_at
                .unwrap_or(item_predicted_start_time + timeplan_entry.duration()),
            planned_start: item_planned_start_time,
            planned_end: item_planned_start_time + timeplan_entry.duration(),
            planned_duration: timeplan_entry.duration().whole_seconds(),
            started_at: entry.started_at,
            ended_at: entry.ended_at,
            timeplan_entry,
        };

        timeplan.items.push(item);
    }

    timeplan.offset = next_offset.or(last_offset).unwrap_or(0);

    Ok(timeplan)
}
