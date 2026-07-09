use crate::json::{json, Json};
use crate::{config::Config, daemon::Daemon, ProjectHealth};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, Response, StatusCode, Uri},
    response::Json as AxumJson,
    routing::{get, post},
    Router,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

pub async fn serve(config: &Config, daemon: Arc<Daemon>) -> crate::error::Result<()> {
    let app = router(daemon);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    info!("dashboard listening on http://{}", config.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(daemon: Arc<Daemon>) -> Router {
    Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/modules", get(modules_handler))
        .route("/api/proposals", get(proposals_handler))
        .route("/api/proposals/{id}/approve", post(approve_handler))
        .route("/api/events", get(events_handler))
        .route("/api/config", get(config_handler))
        .fallback(static_handler)
        .with_state(daemon)
}

async fn health_handler(State(daemon): State<Arc<Daemon>>) -> AxumJson<ProjectHealth> {
    AxumJson(daemon.project_health().await)
}

async fn modules_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    Json(json!({ "modules": daemon.modules().await }))
}

async fn proposals_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    Json(json!({ "proposals": daemon.proposals().await }))
}

async fn approve_handler(
    State(daemon): State<Arc<Daemon>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json, StatusCode> {
    if let Some(proposal) = daemon.proposals().await.into_iter().find(|p| p.id == id) {
        // In a real implementation this would trigger merge.
        Ok(Json(json!({
            "status": "approved",
            "proposal": proposal.id,
        })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn events_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    Json(json!({ "events": daemon.events(50).await }))
}

async fn config_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    let cfg = daemon.config();
    Json(json!({
        "mode": format!("{:?}", cfg.mode),
        "entropy_threshold": cfg.entropy_threshold,
        "confidence_threshold": cfg.confidence_threshold,
        "quiet_period_secs": cfg.quiet_period_secs,
    }))
}

async fn static_handler(uri: Uri) -> Response<Body> {
    let path = uri.path().trim_start_matches('/');
    let file = if path.is_empty() {
        PathBuf::from("static/index.html")
    } else {
        PathBuf::from("static").join(path)
    };

    // Prevent directory traversal outside static/.
    let canonical_file = match tokio::fs::canonicalize(&file).await {
        Ok(c) => c,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("not found"))
                .unwrap()
        }
    };
    let canonical_root = match tokio::fs::canonicalize("static").await {
        Ok(c) => c,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("not found"))
                .unwrap()
        }
    };
    if !canonical_file.starts_with(&canonical_root) {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap();
    }

    match tokio::fs::read(&canonical_file).await {
        Ok(bytes) => {
            let ct = content_type(&canonical_file);
            Response::builder()
                .header(header::CONTENT_TYPE, ct)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn static_handler_serves_index() {
        let uri: Uri = "/".parse().unwrap();
        let resp = static_handler(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn static_handler_blocks_parent_traversal() {
        let uri: Uri = "/../Cargo.toml".parse().unwrap();
        let resp = static_handler(uri).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn static_handler_blocks_encoded_traversal() {
        let uri: Uri = "/../../etc/passwd".parse().unwrap();
        let resp = static_handler(uri).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn static_handler_unknown_path_is_404() {
        let uri: Uri = "/does/not/exist.js".parse().unwrap();
        let resp = static_handler(uri).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
