use std::fs;

use einrad_server::http_server::routes::get_api_openapi;
use tracing::info;
use tracing_subscriber::FmtSubscriber;

fn main() {
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();
    info!("Generating OpenAPI specification");
    let doc = get_api_openapi();
    info!("Writing OpenAPI specification to openapi.json");
    fs::write(
        "openapi.json",
        doc.to_pretty_json().expect("Couldn't generate json"),
    )
    .expect("Couldn't write to file");
    info!("Done");
}
