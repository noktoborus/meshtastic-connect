use axum::Router;
use tower_http::services::ServeDir;

use crate::config::WebConfig;

pub(crate) async fn start(config: WebConfig) -> Result<(), std::io::Error> {
    let app = Router::new().route_service("/", ServeDir::new(config.serve_dir));

    axum_server::bind(config.http_listen.into())
        .serve(app.into_make_service())
        .await
}
