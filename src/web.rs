//! Axum dashboard/API server: JSON endpoints for health, modules,
//! proposals (including approve), config, and events (poll + SSE
//! stream), plus traversal-guarded static files under `static/`.

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

/// Serve the dashboard/API over HTTP on `config.bind`.
///
/// # Errors
/// Returns an error if the bind address cannot be bound or the server fails
/// while serving.
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

/// Default page size for the paginated list endpoints.
const DEFAULT_PAGE_LIMIT: usize = 50;
/// Hard upper bound on a requested page size.
const MAX_PAGE_LIMIT: usize = 500;
/// How far back the events endpoint reads before paginating client-side.
const EVENTS_WINDOW: usize = 1000;

/// Returns `(total, page)` after applying offset/limit (limit clamped to 1..=500).
fn paginate<T>(items: Vec<T>, pg: &Pagination) -> (usize, Vec<T>) {
    let total = items.len();
    let offset = pg.offset.unwrap_or(0).min(total);
    let limit = pg
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
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
    let all = daemon.events(EVENTS_WINDOW).await;
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
    let Ok(canonical_file) = tokio::fs::canonicalize(&file).await else {
        return not_found();
    };
    let Ok(canonical_root) = tokio::fs::canonicalize("static").await else {
        return not_found();
    };
    if !canonical_file.starts_with(&canonical_root) {
        return not_found();
    }

    match tokio::fs::read(&canonical_file).await {
        Ok(bytes) => {
            let ct = content_type(&canonical_file);
            Response::builder()
                .header(header::CONTENT_TYPE, ct)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => not_found(),
    }
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .unwrap()
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

    #[test]
    fn paginate_limit_is_clamped_to_500() {
        let v: Vec<i32> = (0..1_000).collect();
        let (total, page) = paginate(
            v,
            &Pagination {
                limit: Some(10_000),
                offset: None,
            },
        );
        assert_eq!(total, 1_000);
        assert_eq!(page.len(), 500);
    }

    #[test]
    fn paginate_zero_limit_is_clamped_to_one() {
        let (total, page) = paginate(
            vec![7, 8, 9],
            &Pagination {
                limit: Some(0),
                offset: None,
            },
        );
        assert_eq!(total, 3);
        assert_eq!(page, vec![7]);
    }

    #[test]
    fn paginate_defaults_to_first_50() {
        let v: Vec<i32> = (0..120).collect();
        let (total, page) = paginate(v, &Pagination::default());
        assert_eq!(total, 120);
        assert_eq!(page.len(), 50);
        assert_eq!(page[0], 0);
    }

    #[test]
    fn content_type_maps_known_extensions() {
        for (name, expected) in [
            ("a.html", "text/html; charset=utf-8"),
            ("a.js", "application/javascript; charset=utf-8"),
            ("a.css", "text/css; charset=utf-8"),
            ("a.svg", "image/svg+xml"),
            ("a.png", "image/png"),
            ("a.json", "application/json"),
            ("a.wasm", "application/wasm"),
            ("a.bin", "application/octet-stream"),
            ("noext", "application/octet-stream"),
        ] {
            assert_eq!(
                content_type(std::path::Path::new(name)),
                expected,
                "name {name}"
            );
        }
    }

    /// Temp project with one high-entropy module so `scan` yields a proposal.
    fn high_entropy_project() -> PathBuf {
        let dir = temp_project();
        let mut big = String::new();
        for i in 0..60 {
            use std::fmt::Write as _;
            let _ = writeln!(
                big,
                "pub fn f{i}(x: i32) -> i32 {{ if x > 0 {{ if x > 1 {{ if x > 2 {{ x }} else {{ 0 }} }} else {{ 0 }} }} else {{ -1 }}"
            );
        }
        std::fs::write(dir.join("src/big.rs"), &big).unwrap();
        dir
    }

    /// Config whose entropy threshold any indexed module clears, so `scan`
    /// deterministically produces proposals regardless of scoring drift.
    fn permissive_config(root: PathBuf) -> Config {
        let mut cfg = Config::default_for(root);
        cfg.entropy_threshold = 0.1;
        cfg
    }

    #[tokio::test]
    async fn proposals_handler_envelope_lists_proposals() {
        let root = high_entropy_project();
        let daemon = Arc::new(Daemon::new(permissive_config(root.clone())));
        daemon.scan().await.unwrap();
        let expected = daemon.proposals().await.len();
        assert!(expected > 0, "scan should produce a proposal");
        let Json(v) = proposals_handler(State(daemon), Query(Pagination::default())).await;
        let s = v.to_string();
        assert!(s.contains("\"proposals\""), "envelope: {s}");
        assert!(
            s.contains(&format!("\"total\":{expected}")),
            "envelope: {s}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn approve_handler_returns_404_for_unknown_id() {
        let root = temp_project();
        let daemon = Arc::new(Daemon::new(Config::default_for(root.clone())));
        let result = approve_handler(State(daemon), AxumPath("nope".to_string())).await;
        assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn approve_handler_acknowledges_known_proposal() {
        let root = high_entropy_project();
        let daemon = Arc::new(Daemon::new(permissive_config(root.clone())));
        daemon.scan().await.unwrap();
        let proposals = daemon.proposals().await;
        assert!(!proposals.is_empty(), "scan should produce a proposal");
        let id = proposals[0].id.clone();
        let Json(v) = approve_handler(State(daemon), AxumPath(id.clone()))
            .await
            .unwrap();
        let s = v.to_string();
        assert!(s.contains("\"status\":\"approved\""), "body: {s}");
        assert!(s.contains(&id), "body: {s}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn events_handler_envelope_lists_recent_events() {
        let root = temp_project();
        let daemon = Arc::new(Daemon::new(Config::default_for(root.clone())));
        daemon
            .event_bus()
            .emit(crate::EventKind::FileSaved, None)
            .await;
        let Json(v) = events_handler(State(daemon), Query(Pagination::default())).await;
        let s = v.to_string();
        assert!(s.contains("\"events\""), "envelope: {s}");
        assert!(s.contains("\"total\":1"), "envelope: {s}");
        assert!(s.contains("FileSaved"), "envelope: {s}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn config_handler_reports_daemon_config() {
        let root = temp_project();
        let daemon = Arc::new(Daemon::new(Config::default_for(root.clone())));
        let Json(v) = config_handler(State(daemon)).await;
        let s = v.to_string();
        assert!(s.contains("\"mode\":\"passive\""), "body: {s}");
        assert!(s.contains("\"entropy_threshold\":0.82"), "body: {s}");
        assert!(s.contains("\"bind\":\"127.0.0.1:7345\""), "body: {s}");
        assert!(s.contains("\"format\":\"human\""), "body: {s}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
