mod colors;
pub mod command;
pub mod db_backups;
mod etag;
pub mod http_types;
mod logo;
mod photos;
pub mod query;

use axum::{Extension, Router, extract::Request, http::HeaderName, middleware, routing::get};
use axum_embed::{FallbackBehavior, ServeEmbed};
use rust_embed::RustEmbed;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{error, info_span};
use utoipa::{
    OpenApi as OpenApiTrait,
    openapi::{OpenApi, RefOr},
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    image_fetch::ImageFetcher,
    jwt::JWTConfig,
    mailer::Mailer,
    metrics,
    photos::Photos,
    player::{self, PlayerHub},
    reloadable_sqlite::ReloadableSqlite,
};

use super::HttpServerOptions;

const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(RustEmbed, Clone)]
#[folder = "./static/"]
pub(crate) struct StaticAssets;

#[derive(OpenApiTrait)]
#[openapi(
    info(
        title = "Einradevent Login API",
        description = "API for managing registrations",
        license(name = "AGPL-3.0-or-later", url = "https://www.gnu.org/licenses/agpl-3.0.html"),
    ),
    // The messages of the remote playback websocket (`/api/ws/player`),
    // so the frontend gets their types.
    components(schemas(
        player::ControllerMessage,
        player::ToController,
        player::PlayerMessage,
        player::ToPlayer,
        player::HubState,
    )),
    servers(
        (url = "http://localhost:3000"),
        (url = "https://anmeldung.freestyle-cup.nrw"),
    ),
    // security(
    //     (),
    //     ("api_key" = [])
    // ),
)]
pub struct ApiDoc;

fn get_openapi_router() -> (Router, OpenApi) {
    let command_query_router = OpenApiRouter::new()
        .nest("/command", command::get_command_router())
        .nest("/query", query::get_query_router());
    let (router, mut openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api", command_query_router)
        .split_for_parts();
    describe_responses(&mut openapi);
    (router, openapi)
}

/// Gives every response without a description one from its status code.
/// OpenAPI requires a description, and the routes only declare the body.
fn describe_responses(openapi: &mut OpenApi) {
    for item in openapi.paths.paths.values_mut() {
        let operations = [
            &mut item.get,
            &mut item.put,
            &mut item.post,
            &mut item.delete,
            &mut item.options,
            &mut item.head,
            &mut item.patch,
            &mut item.trace,
        ];
        for operation in operations.into_iter().flatten() {
            for (status, response) in &mut operation.responses.responses {
                if let RefOr::T(response) = response
                    && response.description.is_empty()
                {
                    response.description = status_description(status).to_owned();
                }
            }
        }
    }
}

fn status_description(status: &str) -> &'static str {
    match status {
        "200" => "Success",
        "400" => "Invalid request",
        "401" => "Not logged in",
        "403" => "Not allowed",
        "404" => "Not found",
        "409" => "Conflict with the current state",
        "413" => "Too large",
        "500" => "Internal server error",
        _ => "Response",
    }
}

pub fn get_api_router(
    http_options: Arc<HttpServerOptions>,
    db: ReloadableSqlite,
    mailer: Arc<Mailer>,
    jwt_config: Arc<JWTConfig>,
    player_hub: Arc<PlayerHub>,
    photos: Arc<Photos>,
) -> Router {
    let (router, openapi) = get_openapi_router();
    router
        .route("/api/ws/player", get(player::ws::player_ws))
        .route("/api/competition/logo", get(logo::competition_logo))
        .route("/api/competition/break-image", get(logo::break_image))
        .route(
            "/api/competition/sponsors/{id}/image",
            get(logo::sponsor_image),
        )
        .route(
            "/api/competition/colors.css",
            get(colors::competition_colors),
        )
        .route("/api/db-backups/{filename}", get(db_backups::download))
        .route("/api/photos/newest", get(photos::newest))
        .route("/api/photos/random", get(photos::random))
        .route("/api/photos/{id}", get(photos::image))
        .route_layer(middleware::from_fn(metrics::track_http))
        .route("/metrics", get(metrics::metrics_handler))
        .merge(SwaggerUi::new("/swagger").url("/openapi.json", openapi))
        .layer(Extension(db))
        .layer(Extension(mailer))
        .layer(Extension(jwt_config))
        .layer(Extension(http_options))
        .layer(Extension(player_hub))
        .layer(Extension(photos))
        .layer(Extension(ImageFetcher::shared()))
}

#[must_use]
pub fn get_api_openapi() -> OpenApi {
    get_openapi_router().1
}

pub fn get_router(
    http_options: Arc<HttpServerOptions>,
    db: ReloadableSqlite,
    mailer: Arc<Mailer>,
    jwt_config: Arc<JWTConfig>,
    player_hub: Arc<PlayerHub>,
    photos: Arc<Photos>,
) -> Router {
    let request_id_layer = ServiceBuilder::new()
        .layer(SetRequestIdLayer::new(
            HeaderName::from_static(REQUEST_ID_HEADER),
            MakeRequestUuid,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<axum::body::Body>| {
                // Log the request id as generated.
                let request_id = request.headers().get(REQUEST_ID_HEADER);
                let request_path = request.uri().path();

                if let Some(request_id) = request_id {
                    info_span!(
                        "http_request",
                        request_id = ?request_id,
                        request_path = ?request_path,
                    )
                } else {
                    error!("could not extract request_id");
                    info_span!("http_request")
                }
            }),
        )
        // send headers from request to response headers
        .layer(PropagateRequestIdLayer::new(HeaderName::from_static(
            REQUEST_ID_HEADER,
        )));

    let serve_assets = ServeEmbed::<StaticAssets>::with_parameters(
        Some("index.html".into()),
        FallbackBehavior::Ok,
        Some("index.html".into()),
    );

    Router::new()
        .fallback_service(serve_assets)
        .nest_service("/songs", ServeDir::new(http_options.data_path.clone()))
        .merge(get_api_router(
            http_options,
            db,
            mailer,
            jwt_config,
            player_hub,
            photos,
        ))
        .layer(request_id_layer)
}
