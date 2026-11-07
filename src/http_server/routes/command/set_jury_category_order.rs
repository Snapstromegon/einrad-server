use std::collections::HashSet;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetJuryCategoryOrderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJuryCategoryOrderBody {
    /// Every jury category, exactly once, in the new order.
    ids: Vec<Uuid>,
}

/// Set the order of the jury categories. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_category_order",
    request_body=SetJuryCategoryOrderBody,
    responses(
        (status=200, content_type="application/json", body=SetJuryCategoryOrderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_jury_category_order(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SetJuryCategoryOrderBody>,
) -> Result<Json<SetJuryCategoryOrderResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let existing: HashSet<Uuid> =
        sqlx::query_scalar!(r#"SELECT id as "id!: Uuid" FROM jury_categories"#)
            .fetch_all(&mut *transaction)
            .await?
            .into_iter()
            .collect();
    let given: HashSet<Uuid> = body.ids.iter().copied().collect();
    if given.len() != body.ids.len() || given != existing {
        return Err(HttpError::ErrorMessages(
            "Die Reihenfolge muss jede Jury-Kategorie genau einmal enthalten. Bitte neu laden."
                .to_string(),
        ));
    }
    for (index, id) in body.ids.iter().enumerate() {
        let order = index as i64 + 1;
        sqlx::query!(
            "UPDATE jury_categories SET sort_order = ? WHERE id = ?",
            order,
            id
        )
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(Json(SetJuryCategoryOrderResponse {}))
}
