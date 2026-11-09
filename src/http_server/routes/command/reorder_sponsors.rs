use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderSponsorsBody {
    /// All sponsors' ids in the new order.
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReorderSponsorsResponse {}

/// Set the order of the sponsors in the info board's footer.
///
/// `ids` must be all sponsors, each once (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/reorder_sponsors",
    request_body=ReorderSponsorsBody,
    responses(
        (status=200, content_type="application/json", body=ReorderSponsorsResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn reorder_sponsors(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<ReorderSponsorsBody>,
) -> Result<Json<ReorderSponsorsResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let mut current: Vec<i64> = images::list(&mut *tx)
        .await?
        .into_iter()
        .filter(|image| image.kind == "sponsor")
        .map(|image| image.id)
        .collect();
    let mut wanted = body.ids.clone();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err(HttpError::ErrorMessages(
            "Die Reihenfolge muss alle Sponsoren genau einmal enthalten.".to_string(),
        ));
    }
    for (position, id) in (0..).zip(&body.ids) {
        images::set_position(&mut *tx, *id, position).await?;
    }
    tx.commit().await?;
    info!("Reordered the sponsors");
    Ok(Json(ReorderSponsorsResponse {}))
}
