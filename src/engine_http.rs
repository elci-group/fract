//! Hand-rolled HTTP/1.1 over std::net::TcpStream — reqwest is amber-forbidden.
//! http:// only; https:// is rejected with a message pointing at a local TLS
//! terminator.
//!
//! This is the only transport code in the LLM refactor path. Prompt rendering,
//! plan parsing, and output conversion all live in [`crate::prompt`] and are
//! reused verbatim here; this module is responsible solely for shipping the
//! rendered prompt to an OpenAI-compatible `/chat/completions` endpoint and
//! handing the response text back to the parser.

use crate::error::Result;
use crate::json::Value;
use crate::refactor::{RefactorContext, RefactorEngine, RefactorOutput};
use std::future::Future;
use std::pin::Pin;

/// LLM refactor engine backed by an OpenAI-compatible HTTP endpoint.
pub struct HttpRefactorEngine {
    endpoint: String,
    model: String,
    api_key: Option<String>,
    max_tokens: usize,
}

impl HttpRefactorEngine {
    pub fn new(
        endpoint: String,
        model: String,
        api_key: Option<String>,
        max_tokens: usize,
    ) -> Self {
        Self {
            endpoint,
            model,
            api_key,
            max_tokens,
        }
    }
}

impl RefactorEngine for HttpRefactorEngine {
    fn refactor(
        &self,
        ctx: RefactorContext,
    ) -> Pin<Box<dyn Future<Output = Result<RefactorOutput>> + Send + '_>> {
        let endpoint = self.endpoint.clone();
        let model = self.model.clone();
        let api_key = self.api_key.clone();
        let max_tokens = self.max_tokens;
        Box::pin(async move {
            let inner = tokio::task::spawn_blocking(move || -> Result<RefactorOutput> {
                let prompt = crate::prompt::render_prompt(&ctx);
                let body = build_request_body(&model, &prompt, max_tokens);
                let resp = http_post_chat(&endpoint, api_key.as_deref(), &body)?;
                let content = extract_completion_content(&resp)?;
                let plan = crate::prompt::parse_plan(&content)?;
                Ok(crate::prompt::plan_into_output(&ctx, plan))
            })
            .await
            .map_err(|e| -> crate::error::Error { format!("engine task panicked: {e}").into() })?;
            inner
        })
    }
}

fn build_request_body(model: &str, prompt: &str, max_tokens: usize) -> String {
    let mut system_msg = Value::object();
    system_msg.insert("role", Value::String("system".to_string()));
    system_msg.insert(
        "content",
        Value::String(
            "You are a refactoring engine. Respond with exactly one JSON object and no other text."
                .to_string(),
        ),
    );

    let mut user_msg = Value::object();
    user_msg.insert("role", Value::String("user".to_string()));
    user_msg.insert("content", Value::String(prompt.to_string()));

    let mut response_format = Value::object();
    response_format.insert("type", Value::String("json_object".to_string()));

    let mut root = Value::object();
    root.insert("model", Value::String(model.to_string()));
    root.insert("messages", Value::Array(vec![system_msg, user_msg]));
    root.insert("response_format", response_format);
    root.insert("max_tokens", Value::Number(max_tokens as f64));
    root.insert("temperature", Value::Number(0.0));
    root.to_string()
}

fn extract_completion_content(resp_json: &str) -> Result<String> {
    const MISSING: &str = "engine response missing choices[0].message.content";
    let value = crate::json::parse(resp_json).map_err(|e| -> crate::error::Error {
        format!("engine response is not valid JSON: {e}").into()
    })?;
    let root = as_object(&value).ok_or(MISSING)?;
    let choices = get(root, "choices").and_then(as_array).ok_or(MISSING)?;
    let first = choices.first().ok_or(MISSING)?;
    let message = get(as_object(first).ok_or(MISSING)?, "message")
        .and_then(as_object)
        .ok_or(MISSING)?;
    let content = get_str(message, "content").ok_or(MISSING)?;
    Ok(content.to_string())
}

fn as_object(v: &Value) -> Option<&[(String, Value)]> {
    match v {
        Value::Object(entries) => Some(entries),
        _ => None,
    }
}

fn as_array(v: &Value) -> Option<&[Value]> {
    match v {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) => Some(s),
        _ => None,
    }
}

fn get<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn get_str<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    get(obj, key).and_then(as_str)
}

fn parse_endpoint(
    endpoint: &str,
) -> Result<(
    String, /*host*/
    u16,    /*port*/
    String, /*base path*/
)> {
    let e = endpoint.trim();
    if e.starts_with("https://") {
        return Err("https endpoints are not supported by fract's built-in engine; terminate TLS at a local proxy (e.g. a local reverse proxy) and point `llm.endpoint` at http://127.0.0.1:<port>".into());
    }
    let e = e
        .strip_prefix("http://")
        .ok_or("llm.endpoint must start with http:// (https is not supported)")?;
    let (authority, base) = match e.split_once('/') {
        Some((a, b)) => (a, format!("/{}", b.trim_end_matches('/'))),
        None => (e, String::new()),
    };
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| "invalid port in llm.endpoint")?,
        ),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err("llm.endpoint host is empty".into());
    }
    Ok((host, port, if base == "/" { String::new() } else { base }))
}

fn http_post_chat(endpoint: &str, api_key: Option<&str>, body: &str) -> Result<String> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    use std::time::Duration;
    let (host, port, base) = parse_endpoint(endpoint)?;
    let path = format!("{base}/chat/completions");
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {host}:{port}: {e}"))?
        .next()
        .ok_or_else(|| format!("no address for {host}:{port}"))?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| format!("connect {host}:{port} failed: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(k) = api_key {
        head.push_str(&format!("Authorization: Bearer {k}\r\n"));
    }
    stream.write_all(head.as_bytes())?;
    stream.write_all(b"\r\n")?;
    stream.write_all(body.as_bytes())?;
    // Contract is `Connection: close` + read_to_end, so chunked transfer
    // decoding is not required for OpenAI-compatible local endpoints.
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("read response failed: {e}"))?;
    let text = String::from_utf8(raw).map_err(|_| "engine response was not valid UTF-8")?;
    let (hdr, rsp_body) = text
        .split_once("\r\n\r\n")
        .ok_or("malformed HTTP response (no header/body split)")?;
    let status_line = hdr.lines().next().unwrap_or("");
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    if !(200..300).contains(&code) {
        let snippet: String = rsp_body.chars().take(300).collect();
        return Err(format!("engine endpoint returned {status_line}: {snippet}").into());
    }
    Ok(rsp_body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refactor::RefactorContext;
    use crate::time::now;
    use crate::{Health, Language, Module};
    use std::io::{Read, Write};
    use std::path::PathBuf;

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn sample_ctx() -> RefactorContext {
        RefactorContext {
            module: Module {
                path: PathBuf::from("src/foo.rs"),
                language: Language::Rust,
                lines: 1,
                functions: 1,
                cyclomatic_complexity: 0,
                public_api_size: 0,
                fan_out: 0,
                fan_in: 0,
                duplicates: 0,
                edit_frequency: 0.0,
                confidence: None,
                churn: 0,
                test_coverage: 0.0,
                entropy: 0.9,
                health: Health::Healthy,
                last_modified: now(),
            },
            source: "fn old(){}\n".to_string(),
            imports: Vec::new(),
            exports: Vec::new(),
            dependents: Vec::new(),
            project_conventions: String::new(),
        }
    }

    const PLAN: &str = r#"{"rationale":"split","migration_notes":["note"],"files":[{"path":"src/foo.rs","content":"fn a(){}\n"}]}"#;

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// Read one HTTP/1.1 request: headers up to `\r\n\r\n`, then exactly
    /// `Content-Length` body bytes. Returns (header block, body).
    fn read_request(stream: &mut std::net::TcpStream) -> (String, String) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        let header_end;
        loop {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "client closed before sending headers");
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }
        let header = String::from_utf8(buf[..header_end].to_vec()).unwrap();
        let mut content_length = 0usize;
        for line in header.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("content-length:") {
                content_length = rest.trim().parse().unwrap();
            }
        }
        let mut body = buf[header_end..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "client closed before sending full body");
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(content_length);
        (header, String::from_utf8(body).unwrap())
    }

    #[test]
    fn posts_chat_completions_and_parses_plan() {
        let (tx, rx) = std::sync::mpsc::channel();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (header, body) = read_request(&mut stream);
            tx.send((header, body)).unwrap();
            let escaped = PLAN
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n");
            let body = format!(
                r#"{{"choices":[{{"message":{{"role":"assistant","content":"{escaped}"}}}}]}}"#
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
        });

        let engine = HttpRefactorEngine::new(format!("http://{addr}"), "m".into(), None, 1024);
        let out = run(engine.refactor(sample_ctx())).unwrap();

        let (header, req_body) = rx.recv().unwrap();
        let request_line = header.lines().next().unwrap_or("");
        assert!(
            request_line.starts_with("POST "),
            "request line: {request_line}"
        );
        assert!(
            request_line.contains("/chat/completions"),
            "request line: {request_line}"
        );
        assert!(
            req_body.contains("\"response_format\""),
            "body missing response_format: {req_body}"
        );
        assert!(
            req_body.contains("\"m\""),
            "body missing model name: {req_body}"
        );

        assert!(
            out.files
                .iter()
                .any(|(p, c)| p.to_str() == Some("src/foo.rs") && c.contains("fn a()")),
            "output files did not round-trip the plan: {:?}",
            out.files
        );
        assert_eq!(out.migration_notes, vec!["note".to_string()]);

        handle.join().unwrap();
    }

    #[test]
    fn non_2xx_yields_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\nboom",
                )
                .unwrap();
        });

        let engine = HttpRefactorEngine::new(format!("http://{addr}"), "m".into(), None, 1024);
        let res = run(engine.refactor(sample_ctx()));
        assert!(res.is_err(), "expected non-2xx to error, got Ok");

        handle.join().unwrap();
    }

    #[test]
    fn https_endpoint_is_rejected_without_connecting() {
        let engine =
            HttpRefactorEngine::new("https://api.example/v1".into(), "m".into(), None, 1024);
        let res = run(engine.refactor(sample_ctx()));
        let err = match res {
            Ok(_) => panic!("expected https endpoint to be rejected"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("https"), "error should mention https: {msg}");
        assert!(
            msg.contains("TLS") || msg.contains("terminat"),
            "error should point at a TLS terminator: {msg}"
        );
    }
}
