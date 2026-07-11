//! Live end-to-end test: boot the real axum server on an ephemeral port against
//! a real temp git repo, hit REST endpoints over real TCP, and consume one
//! Server-Sent Events frame from `/api/events/stream`.
//!
//! Hand-rolled HTTP/1.1 over `std::net::TcpStream` only — no `reqwest`, no
//! `tempfile` (both are forbidden by `.amber.toml`). The temp directory uses
//! `std::env::temp_dir()` + a unique suffix and `git init` for repo fidelity.

use fract::config::Config;
use fract::daemon::Daemon;
use std::sync::Arc;

fn http_get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut s =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    // Read the full response. `Connection: close` makes the server drop the
    // socket at end-of-body (Ok(0)); if it lingers, a read timeout AFTER we
    // have already captured the header block also ends the read cleanly — so
    // the helper never false-fails on the benign EOF-probe read.
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut have_headers = false;
    loop {
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if !have_headers && buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    have_headers = true;
                }
            }
            Err(ref e)
                if (e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut)
                    && have_headers =>
            {
                break;
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                panic!("timed out waiting for response headers");
            }
            Err(e) => panic!("HTTP read error: {e}"),
        }
    }
    let text = String::from_utf8(buf).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code: u16 = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (code, body.to_string())
}

fn read_first_sse_data(addr: std::net::SocketAddr, ready: std::sync::mpsc::Sender<()>) -> String {
    use std::io::{Read, Write};
    let mut s =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(8)))
        .unwrap();
    let req = format!(
        "GET /api/events/stream HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n\r\n"
    );
    s.write_all(req.as_bytes()).unwrap();
    // Read until a `data:` line appears. Once the response headers have been
    // received the server-side handler has already run `event_bus().subscribe()`,
    // so we signal readiness and the caller emits exactly one event afterwards —
    // a deterministic subscribe-before-emit handshake instead of a racy sleep.
    let mut acc = Vec::new();
    let mut tmp = [0u8; 1024];
    let mut signalled = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for SSE data frame");
        }
        match s.read(&mut tmp) {
            Ok(0) => panic!("SSE stream closed before any data frame"),
            Ok(n) => {
                acc.extend_from_slice(&tmp[..n]);
                let text = String::from_utf8_lossy(&acc);
                if let Some(idx) = text.find("\r\n\r\n") {
                    if !signalled {
                        let _ = ready.send(());
                        signalled = true;
                    }
                    // past headers
                    let after = &text[idx..];
                    for line in after.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            return data.trim().to_string();
                        }
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => panic!("SSE read error: {e}"),
        }
    }
}

fn temp_git_repo() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fract-live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
    let git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
        assert!(st.success(), "git {:?} failed", args);
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    dir
}

#[tokio::test]
async fn live_daemon_serves_rest_and_one_sse_event() {
    let root = temp_git_repo();
    let mut cfg = Config::default_for(root.clone());
    cfg.bind = "127.0.0.1:0".to_string(); // ignored; we bind our own listener below
    cfg.llm.provider = "mock".to_string();
    let daemon = Arc::new(Daemon::new(cfg));
    daemon.scan().await.unwrap();

    let app = fract::web::router(daemon.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // REST over real TCP: health + modules carry the unified envelope.
    // The client is blocking std I/O, so run it on the blocking pool — otherwise
    // it would occupy the async worker and starve the spawned axum server.
    let (hc, hbody) = tokio::task::spawn_blocking(move || http_get(addr, "/api/health"))
        .await
        .unwrap();
    assert_eq!(hc, 200, "health status");
    assert!(
        hbody.contains("fract.report/v1") && hbody.contains("\"health\""),
        "health envelope: {hbody}"
    );
    let (mc, mbody) =
        tokio::task::spawn_blocking(move || http_get(addr, "/api/modules?limit=5&offset=0"))
            .await
            .unwrap();
    assert_eq!(mc, 200, "modules status");
    assert!(
        mbody.contains("\"total\"") && mbody.contains("\"modules\""),
        "modules envelope: {mbody}"
    );

    // SSE: connect, wait until the server has subscribed (headers received),
    // then emit exactly one event and read one data frame. The readiness
    // handshake removes the subscribe/emit race without any unbounded loop.
    //
    // Both the readiness wait and the reader join are blocking std calls, so
    // they run on the blocking pool — keeping the async worker free to poll the
    // spawned axum server (which would otherwise be starved on the test runtime).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let sse = std::thread::spawn(move || read_first_sse_data(addr, ready_tx));
    let subscribed = tokio::task::spawn_blocking(move || {
        ready_rx.recv_timeout(std::time::Duration::from_secs(5))
    })
    .await
    .unwrap();
    subscribed.expect("SSE stream did not subscribe in time");
    daemon
        .event_bus()
        .emit(fract::EventKind::EditorHeartbeat, None)
        .await;
    let data = tokio::task::spawn_blocking(move || sse.join())
        .await
        .unwrap()
        .expect("sse thread panicked");
    assert!(
        data.contains("\"kind\"") || data.contains("EditorHeartbeat") || data.contains("\"type\""),
        "SSE data frame should carry the event, got: {data}"
    );

    server.abort();
    let _ = std::fs::remove_dir_all(&root);
}
