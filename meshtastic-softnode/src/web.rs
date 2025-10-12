use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing};
use futures::StreamExt;
use rustls_acme::AcmeConfig;
use rustls_acme::axum::AxumAcceptor;
use rustls_acme::caches::DirCache;
use serde::Deserialize;
use tower_http::cors;
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
    let cors = cors::CorsLayer::new()
        .allow_origin(cors::Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    let acme = if let Some(acme) = config.tls_acme {
        let acme_state = AcmeConfig::new(acme.domains)
            .contact(acme.emails.iter().map(|e| format!("mailto:{}", e)))
            .cache(DirCache::new(acme.cache_dir))
            .directory_lets_encrypt(acme.is_prod)
            .state();

        Some(acme_state)
    } else {
        None
    };

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
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    if let Some(mut acme_state) = acme {
        let acceptor = acme_state.axum_acceptor(acme_state.default_rustls_config());

        tokio::spawn(async move {
            loop {
                match acme_state.next().await.unwrap() {
                    Ok(ok) => println!("tlsacme event: {:?}", ok),
                    Err(err) => println!("tlsacme error: {:?}", err),
                }
            }
        });

        axum_server::bind(config.http_listen.into())
            .acceptor(acceptor)
            .serve(app.into_make_service())
            .await
    } else {
        axum_server::bind(config.http_listen.into())
            .serve(app.into_make_service())
            .await
    }
}
