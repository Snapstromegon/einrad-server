use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema, Clone)]
pub struct StartlistActParticipant {
    firstname: String,
    lastname: String,
    id: Uuid,
    club_name: String,
    /// Replacement riders (Ersatzfahrer) of a group.
    is_replacement: bool,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct StartlistAct {
    id: Uuid,
    name: String,
    /// Group acts only.
    group_name: Option<String>,
    act_type: ActType,
    participants: Vec<StartlistActParticipant>,
    category: Option<String>,
    act_order: Option<i64>,
    category_order: Option<i64>,
}

/// The public startlist: all acts with a category, in start order.
///
/// Public, so it leaves out the riders' ages and club ids.
#[utoipa::path(
    get,
    tags=["query", "acts"],
    path="/startlist",
    responses(
        (status=200, content_type="application/json", body=Vec<StartlistAct>),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn startlist(
    Extension(db): Extension<ReloadableSqlite>,
) -> Result<Json<Vec<StartlistAct>>, HttpError> {
    pub struct DBStartlistAct {
        id: Uuid,
        name: String,
        group_name: Option<String>,
        act_type: ActType,
        participants: sqlx::types::Json<Vec<StartlistActParticipant>>,
        category: Option<String>,
        act_order: Option<i64>,
        category_order: Option<i64>,
    }
    impl From<DBStartlistAct> for StartlistAct {
        fn from(db_act: DBStartlistAct) -> Self {
            StartlistAct {
                id: db_act.id,
                name: db_act.name,
                group_name: db_act.group_name,
                act_type: db_act.act_type,
                participants: db_act.participants.0,
                category: db_act.category,
                act_order: db_act.act_order,
                category_order: db_act.category_order,
            }
        }
    }
    let db = db.get().await.clone();
    let acts = sqlx::query_as!(
        DBStartlistAct,
        r#"
        SELECT
            view_act.id as "id!: Uuid",
            view_act."order" as "act_order",
            categories."order" as "category_order",
            view_act.name,
            view_act.group_name,
            view_act.act_type as "act_type!: ActType",
            view_act.participants as "participants!: sqlx::types::Json<Vec<StartlistActParticipant>>",
            view_act.category
        FROM view_act JOIN categories ON view_act.category = categories.name
        ORDER BY categories."order", view_act."order" ASC
        "#
    )
    .fetch_all(&db)
    .await?;
    Ok(Json(acts.into_iter().map(|a| a.into()).collect()))
}
