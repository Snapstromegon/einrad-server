use crate::ids::{ClubId, UserId};
use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{
        HttpError,
        extractor::auth::Auth,
        routes::http_types::{CurrentUser, User},
    },
    reloadable_sqlite::ReloadableSqlite,
};

/// Get information about the currently authenticated user.
#[utoipa::path(
    get,
    tags=["query", "auth"],
    path="/whoami",
    responses(
        (status=200, content_type="application/json", body=CurrentUser),
    ),
)]
#[instrument(skip(db))]
pub async fn whoami(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
) -> Result<Json<CurrentUser>, HttpError> {
    let db = db.get().await.clone();
    let user = sqlx::query!(
        r#"
        SELECT id as "id!: UserId", name, email, email_verified, club_id as "club_id: ClubId" FROM users WHERE id = $1
        "#,
        auth.user_id
    )
    .fetch_one(&db)
    .await?;
    Ok(Json(CurrentUser {
        user: User {
            id: user.id,
            name: user.name,
            email: user.email,
            email_verified: user.email_verified,
            roles: auth.roles.clone(),
            club_id: user.club_id,
        },
        permissions: auth.permissions(),
    }))
}
