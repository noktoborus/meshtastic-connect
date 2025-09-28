use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing};
use serde::Deserialize;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::config::WebConfig;
use crate::sqlite::SQLite;

use tracing_subscriber::EnvFilter;

pub(crate) fn init() {
    tracing_subscriber::fmt()
        // This allows you to use, e.g., `RUST_LOG=info` or `RUST_LOG=debug`
        // when running the app to set log levels.
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new("tower_http=debug"))
                .unwrap(),
        )
        .init();
}

#[derive(Clone)]
struct Web {
    pub sqlite: SQLite,
}

#[derive(Deserialize)]
struct SyncParams {
    start: Option<u64>,
}

async fn api_softnode(
    State(state): State<Arc<Web>>,
    params: Query<SyncParams>,
) -> (
    StatusCode,
    Json<Vec<softnode_client::app::data::StoredMeshPacket>>,
) {
    const SELECT_LIMIT: usize = 100;
    if let Ok(packets) = state
        .sqlite
        .select_packets(params.start, SELECT_LIMIT)
        .await
    {
        (StatusCode::OK, Json(packets))
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(Vec::new()))
    }
}

pub(crate) async fn start(config: WebConfig, sqlite: SQLite) -> Result<(), std::io::Error> {
    let state = Arc::new(Web { sqlite });

    let app = Router::new()
        .fallback_service(ServeDir::new(config.serve_dir))
        .nest(
            "/api",
            Router::new().nest(
                "/softnode",
                Router::new().route("/sync", routing::get(api_softnode)),
            ),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    axum_server::bind(config.http_listen.into())
        .serve(app.into_make_service())
        .await
}
