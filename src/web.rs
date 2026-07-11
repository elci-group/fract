use crate::json::{to_value, Json, Value};
use crate::report::SCHEMA;
use crate::{config::Config, daemon::Daemon};
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, Response, StatusCode, Uri},
    response::sse::{Event as SseEvent, Sse},
    routing::{get, post},
    Router,
};
use futures_util::stream::{unfold, Stream};
use serde::Deserialize;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

pub async fn serve(config: &Config, daemon: Arc<Daemon>) -> crate::error::Result<()> {
    let app = router(daemon);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    info!(event = "web.listen", bind = %config.bind, "dashboard listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the axum Router for the daemon. Public so the server can be embedded and exercised end-to-end by integration tests; `serve` is the production entry point and calls this.
pub fn router(daemon: Arc<Daemon>) -> Router {
    Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/modules", get(modules_handler))
        .route("/api/proposals", get(proposals_handler))
        .route("/api/proposals/{id}/approve", post(approve_handler))
        .route("/api/events", get(events_handler))
        .route("/api/events/stream", get(events_stream_handler))
        .route("/api/config", get(config_handler))
        .fallback(static_handler)
        .with_state(daemon)
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    limit: Option<usize>,
    offset: Option<usize>,
}

/// Returns `(total, page)` after applying offset/limit (limit clamped to 1..=500).
fn paginate<T>(items: Vec<T>, pg: &Pagination) -> (usize, Vec<T>) {
    let total = items.len();
    let offset = pg.offset.unwrap_or(0).min(total);
    let limit = pg.limit.unwrap_or(50).clamp(1, 500);
    let page = items.into_iter().skip(offset).take(limit).collect();
    (total, page)
}

fn schema_object() -> Value {
    let mut o = Value::object();
    o.insert("schema", Value::String(SCHEMA.to_string()));
    o
}

async fn health_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    let mut o = schema_object();
    o.insert("health", to_value(daemon.project_health().await));
    Json(o)
}

async fn modules_handler(State(daemon): State<Arc<Daemon>>, Query(pg): Query<Pagination>) -> Json {
    let mut all = daemon.modules().await;
    all.sort_by(|a, b| {
        b.entropy
            .partial_cmp(&a.entropy)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    let (total, page) = paginate(all, &pg);
    let mut o = schema_object();
    o.insert("total", Value::Number(total as f64));
    o.insert("modules", to_value(page));
    Json(o)
}

async fn proposals_handler(
    State(daemon): State<Arc<Daemon>>,
    Query(pg): Query<Pagination>,
) -> Json {
    let all = daemon.proposals().await;
    let (total, page) = paginate(all, &pg);
    let mut o = schema_object();
    o.insert("total", Value::Number(total as f64));
    o.insert("proposals", to_value(page));
    Json(o)
}

async fn approve_handler(
    State(daemon): State<Arc<Daemon>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json, StatusCode> {
    if let Some(proposal) = daemon.proposals().await.into_iter().find(|p| p.id == id) {
        // In a real implementation this would trigger merge.
        let mut o = schema_object();
        o.insert("status", Value::String("approved".to_string()));
        o.insert("proposal", Value::String(proposal.id));
        Ok(Json(o))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn events_handler(State(daemon): State<Arc<Daemon>>, Query(pg): Query<Pagination>) -> Json {
    // Pull a deep recent window, then paginate client-side.
    let all = daemon.events(1000).await;
    let (total, page) = paginate(all, &pg);
    let mut o = schema_object();
    o.insert("total", Value::Number(total as f64));
    o.insert("events", to_value(page));
    Json(o)
}

async fn events_stream_handler(
    State(daemon): State<Arc<Daemon>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = daemon.event_bus().subscribe();
    let stream = unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(ev) => {
                let data = to_value(ev).to_string();
                Some((Ok(SseEvent::default().event("event").data(data)), rx))
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                let data = format!("{{\"lagged\":{n}}}");
                Some((Ok(SseEvent::default().event("lag").data(data)), rx))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(stream)
}

async fn config_handler(State(daemon): State<Arc<Daemon>>) -> Json {
    let cfg = daemon.config();
    let mut output = Value::object();
    output.insert("format", Value::String(cfg.output.format.clone()));
    output.insert("color", Value::String(cfg.output.color.clone()));
    output.insert("verbosity", Value::String(cfg.output.verbosity.clone()));
    output.insert(
        "max_findings",
        Value::Number(cfg.output.max_findings as f64),
    );

    let mut config = Value::object();
    config.insert("mode", Value::String(cfg.mode.to_string()));
    config.insert("entropy_threshold", Value::Number(cfg.entropy_threshold));
    config.insert(
        "confidence_threshold",
        Value::Number(cfg.confidence_threshold),
    );
    config.insert(
        "quiet_period_secs",
        Value::Number(cfg.quiet_period_secs as f64),
    );
    config.insert("bind", Value::String(cfg.bind.clone()));
    config.insert("output", output);

    let mut o = schema_object();
    o.insert("config", config);
    Json(o)
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

    #[test]
    fn paginate_clamps_and_slices() {
        let v: Vec<i32> = (0..10).collect();
        let (total, page) = paginate(
            v,
            &Pagination {
                limit: Some(3),
                offset: Some(4),
            },
        );
        assert_eq!(total, 10);
        assert_eq!(page, vec![4, 5, 6]);
    }

    #[test]
    fn paginate_offset_past_end_is_empty() {
        let (total, page) = paginate(
            vec![1, 2],
            &Pagination {
                limit: Some(10),
                offset: Some(99),
            },
        );
        assert_eq!(total, 2);
        assert!(page.is_empty());
    }

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fract-web-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        dir
    }

    #[tokio::test]
    async fn modules_handler_envelope_has_schema_total_modules() {
        let root = temp_project();
        let daemon = Arc::new(Daemon::new(Config::default_for(root.clone())));
        daemon.scan().await.unwrap();
        let Json(v) = modules_handler(State(daemon), Query(Pagination::default())).await;
        let s = v.to_string();
        assert!(s.contains(SCHEMA), "envelope carries schema: {s}");
        assert!(s.contains("\"total\""), "envelope carries total: {s}");
        assert!(s.contains("\"modules\""), "envelope carries modules: {s}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn health_handler_uses_single_json_path() {
        let root = temp_project();
        let daemon = Arc::new(Daemon::new(Config::default_for(root.clone())));
        daemon.scan().await.unwrap();
        let Json(v) = health_handler(State(daemon)).await;
        let s = v.to_string();
        assert!(s.contains("\"health\""));
        assert!(s.contains(SCHEMA));
        let _ = std::fs::remove_dir_all(&root);
    }
}
