use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted, routes::http_types::User},
    reloadable_sqlite::ReloadableSqlite,
    roles::{self, permit},
};

/// List all users.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/list_users",
    responses(
        (status=200, content_type="application/json", body=Vec<User>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_users(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageUsers>,
) -> Result<Json<Vec<User>>, HttpError> {
    let db = db.get().await.clone();
    let mut roles = roles::all_roles(&db).await?;
    let users = sqlx::query!(
        r#"
        SELECT id as "id!: Uuid", club_id as "club_id: Uuid", name, email, email_verified FROM users
        "#
    )
    .fetch_all(&db)
    .await?
    .into_iter()
    .map(|user| {
        let roles = roles.remove(&user.id).unwrap_or_default();
        User {
            id: user.id,
            name: user.name,
            email: user.email,
            email_verified: user.email_verified,
            roles,
            club_id: user.club_id,
        }
    })
    .collect();
    Ok(Json(users))
}
