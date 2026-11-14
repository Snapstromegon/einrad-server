use crate::ids::{JudgeId, JuryUnitId};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JuryEnabled},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    seed::{RemapFile, RemapRegistration},
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyJuryRemapBody {
    /// The remap file's content (TOML) with `jury_unit` filled in.
    file: String,
}

/// Why an entry of the remap file wasn't registered.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemapProblem {
    /// No judge (or more than one) of that name in that club.
    JudgeNotFound,
    /// No jury unit of that name (`heat: <name>` or `final: <name>`).
    JuryUnitNotFound,
    /// The jury unit takes no registrations (D6): its Vorlauf is skipped or
    /// the jury lead switched them off.
    JuryUnitNotRegistrable,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RemapEntryNotFound {
    registration: RemapRegistration,
    problem: RemapProblem,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApplyJuryRemapResponse {
    /// Registrations made again (or already there).
    registered: usize,
    /// Entries without a `jury_unit`; nothing was done for them.
    unfilled: Vec<RemapRegistration>,
    /// Entries whose judge or jury unit isn't found, or whose jury unit takes
    /// no registrations.
    not_found: Vec<RemapEntryNotFound>,
}

/// Applies a remap file of judge registrations a replace dropped (ADR
/// 0005): every entry with a filled-in, existing `jury_unit` (`heat: <name>`
/// or `final: <name>`) registers its judge for that jury unit and task
/// again. Entries whose judge or jury unit isn't found, or whose jury unit
/// takes no registrations (D6), are listed back. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/apply_jury_remap",
    request_body=ApplyJuryRemapBody,
    responses(
        (status=200, content_type="application/json", body=ApplyJuryRemapResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn apply_jury_remap(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<ApplyJuryRemapBody>,
) -> Result<Json<ApplyJuryRemapResponse>, HttpError> {
    let remap: RemapFile = toml::from_str(&body.file)
        .map_err(|e| HttpError::ErrorMessages(format!("Ungültige Remap-Datei: {e}")))?;
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let mut registered = 0;
    let mut unfilled = Vec::new();
    let mut not_found = Vec::new();
    for registration in remap.jury_registrations {
        let label = registration.jury_unit.trim();
        if label.is_empty() {
            unfilled.push(registration);
            continue;
        }
        let judges = sqlx::query_scalar!(
            r#"
            SELECT judge.id as "id!: JudgeId" FROM judge
            JOIN clubs ON clubs.id = judge.club_id
            WHERE clubs.name = $1 AND judge.firstname = $2 AND judge.lastname = $3
            "#,
            registration.club,
            registration.firstname,
            registration.lastname,
        )
        .fetch_all(&mut *transaction)
        .await?;
        let [judge] = judges[..] else {
            not_found.push(RemapEntryNotFound {
                registration,
                problem: RemapProblem::JudgeNotFound,
            });
            continue;
        };
        let unit = match jury::parse_unit_label(label) {
            Some((kind, name)) => sqlx::query!(
                r#"SELECT id as "id!: JuryUnitId" FROM view_jury_unit WHERE kind = $1 AND name = $2"#,
                kind,
                name
            )
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| (kind, row.id)),
            None => None,
        };
        let Some((kind, unit)) = unit else {
            not_found.push(RemapEntryNotFound {
                registration,
                problem: RemapProblem::JuryUnitNotFound,
            });
            continue;
        };
        if jury::unit_is_registrable(&mut transaction, kind, unit).await? != Some(true) {
            not_found.push(RemapEntryNotFound {
                registration,
                problem: RemapProblem::JuryUnitNotRegistrable,
            });
            continue;
        }
        let (heat_id, final_id) = jury::unit_columns(kind, unit);
        sqlx::query!(
            r#"
            INSERT INTO judge_registration (judge_id, heat_id, final_id, task) VALUES ($1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
            judge,
            heat_id,
            final_id,
            registration.task,
        )
        .execute(&mut *transaction)
        .await?;
        registered += 1;
    }
    transaction.commit().await?;
    info!(
        "Applied a remap file: {registered} registered, {} unfilled, {} not found",
        unfilled.len(),
        not_found.len()
    );
    Ok(Json(ApplyJuryRemapResponse {
        registered,
        unfilled,
        not_found,
    }))
}
