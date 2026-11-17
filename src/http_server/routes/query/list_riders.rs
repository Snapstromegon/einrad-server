use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

pub use db_core::riders::Rider;

/// Every rider with their club and starts; birthdates and ages only for
/// who may see them.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_riders",
    responses(
        (status=200, content_type="application/json", body=Vec<Rider>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_riders(
    Extension(db): Extension<DbHandle>,
    _: Granted<permit::ViewRiders>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
) -> Result<Json<Vec<Rider>>, HttpError> {
    let club_riders = db
        .list_riders()
        .await?
        .into_iter()
        .map(|rider| {
            if birthdates.is_some() {
                rider
            } else {
                Rider {
                    birthdate: None,
                    age_on_competition: None,
                    ..rider
                }
            }
        })
        .collect();
    Ok(Json(club_riders))
}
