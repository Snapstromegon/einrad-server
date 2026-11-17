use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryEnabled, RemapOutcome, RemapProblem, RemapRegistration},
    roles::permit,
    seed::RemapFile,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyJuryRemapBody {
    /// The remap file's content (TOML) with `jury_unit` filled in.
    file: String,
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
#[instrument(skip(repo, body))]
#[axum::debug_handler]
pub async fn apply_jury_remap(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<ApplyJuryRemapBody>,
) -> Result<Json<ApplyJuryRemapResponse>, HttpError> {
    let remap: RemapFile = toml::from_str(&body.file)
        .map_err(|e| HttpError::ErrorMessages(format!("Ungültige Remap-Datei: {e}")))?;
    let RemapOutcome {
        registered,
        unfilled,
        not_found,
    } = repo.apply_jury_remap(remap.jury_registrations).await?;
    let not_found: Vec<_> = not_found
        .into_iter()
        .map(|(registration, problem)| RemapEntryNotFound {
            registration,
            problem,
        })
        .collect();
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
