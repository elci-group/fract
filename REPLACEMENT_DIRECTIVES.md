# Fract Dependency Replacement Directives

Generated from `amber` analysis and source review. Goal: replace easily
vendored third-party crates with stdlib or internal modules, improving
compile times, binary size, and runtime performance while preserving the
existing public API shape.

## Baseline

- Project: `/home/sal/fract`
- Rust edition: 2021
- rustc: 1.96.1
- Direct dependencies: 23
- amber-recommended replacement crates (score ≥ 60):
  - chrono (80), thiserror (76), anyhow (72), clap (68), walkdir (67),
    git2 (65), serde_json (65), regex (64), async-trait (63), hex (63),
    tokio-test (63), tower-http (63), tracing-subscriber (63), tempfile (62),
    toml (61), tracing (58), axum (57), notify (57)

## Scope Decision

Full framework replacements (axum, notify, tokio, git2, serde, toml) are out
of scope for a single iteration: they are security-blocked or low-score and
drive core behaviour. The following crates are targeted because they are
small-surface, localized, and replaceable with stdlib/internal code without
rewriting architecture.

| # | Crate | Score | Location(s) | Replacement Strategy |
|---|-------|-------|-------------|----------------------|
| 1 | `walkdir` | 67 | `src/indexer.rs` | Internal recursive `std::fs::read_dir` walker |
| 2 | `regex` | 64 | `src/indexer.rs` | Hand-written line scanners / token counters |
| 3 | `anyhow` | 72 | 9 files, 30 call sites | Internal `fract::Error` enum + `Context` trait |
| 4 | `chrono` | 80 | 8 files | `std::time::SystemTime` + RFC 3339 formatting |
| 5 | `serde_json` | 65 | `src/web.rs` | Lightweight `JsonValue` builder + serializer |
| 6 | `clap` | 68 | `src/main.rs` | Manual argument parser |
| 7 | `uuid` | 20 | `src/queue.rs` | Counter-based deterministic IDs |
| 8 | `tempfile` | 62 | `src/daemon.rs` | `std::env::temp_dir()` + random directory |
| 9 | `async-trait` | 63 | `src/refactor.rs` | Native async trait (Rust 1.75+) |
| 10 | `tower-http` | 63 | `src/web.rs` | Axum built-ins / manual CORS + static service |
| 11 | unused | – | – | Remove `hex`, `thiserror`, `sha2`, `reqwest`, `tokio-test` |

## Directive 1 — Replace `walkdir` with `fract::walk`

### Why
`walkdir` pulls a small dep tree but is only used to recursively enumerate
files while respecting ignore patterns. A stdlib walker is trivial, avoids
`Send + Sync` trait overhead, and lets us fuse ignore-filtering with
filesystem traversal.

### Interface
Create `src/walk.rs`:

```rust
pub struct Walk { root: PathBuf, ignore: Vec<String> }
pub struct Entry { path: PathBuf, is_file: bool, is_dir: bool }

impl Walk {
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self;
    pub fn files(self) -> impl Iterator<Item = io::Result<PathBuf>>;
}
```

### Behaviour
- Recurse with `std::fs::read_dir`.
- Skip entries matching any ignore glob using existing `Indexer::glob_match`.
- Skip symlink loops by tracking visited `(dev, ino)` pairs.
- Surface `io::Error` per entry rather than failing the whole walk.

### Migration
In `src/indexer.rs`:
- Replace `WalkDir::new(&self.root).into_iter().filter_entry(...)` with
  `Walk::new(self.root.clone(), self.ignore.clone()).files()`.
- Remove `use walkdir::WalkDir;`.

### Performance target
- Indexing a 1 000-file tree ≥ 1.2× faster than `walkdir` baseline.
- Zero allocations per yielded path beyond the `PathBuf` itself.

### Validation
- `cargo test` passes.
- New unit test: walker respects ignore patterns and finds expected files.

---

## Directive 2 — Replace `regex` with hand-written scanners

### Why
`indexer.rs` compiles fresh regexes on every file analysis and uses them for
simple prefix/keyword counting. This is a hotspot and a pure performance win.

### Interface
Create `src/scanner.rs`:

```rust
pub struct RustScanner;
pub struct PythonScanner;
pub struct JsTsScanner;

impl RustScanner {
    pub fn count_functions(text: &str) -> usize;
    pub fn count_branches(text: &str) -> usize;
    pub fn count_public_items(text: &str) -> usize;
    pub fn count_imports(text: &str) -> usize;
}
```

### Behaviour
- Function detection: line starts with optional `pub `/`async `/`unsafe ` then `fn `.
- Branch detection: whole-word matching of `if|else|match|while|for|loop|||&&`.
- Public API: lines starting with `pub `.
- Imports: lines starting with `use `.
- Preserve glob-to-regex helper only for ignore patterns, or replace it with a
  simple glob matcher in `src/glob.rs`.

### Migration
- Replace all `Regex::new(...).find_iter(...)` calls in `indexer.rs` with
  scanner functions.
- Remove `use regex::Regex;`.

### Performance target
- `cargo test --lib` indexer path ≥ 2× faster on a 10 000-line synthetic file.
- Eliminate regex compilation overhead in hot path.

### Validation
- Existing tests still pass.
- Add scanner unit tests covering Rust/Python/JS-TS samples.

---

## Directive 3 — Replace `anyhow` with `fract::Error`

### Why
`anyhow` is used for ergonomic error propagation. A small internal error type
removes dynamic dispatch, shrinks the dependency tree, and keeps the same
`?` ergonomics via `From` impls and a `Context` trait.

### Interface
Create `src/error.rs`:

```rust
#[derive(Debug)]
pub struct Error { message: String, source: Option<Box<dyn std::error::Error + Send + Sync>> }

pub type Result<T> = std::result::Result<T, Error>;

pub trait Context<T> {
    fn context(self, msg: impl Into<String>) -> Result<T>;
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> Context<T> for std::result::Result<T, E> { ... }
```

### Behaviour
- `Error` implements `std::error::Error`, `Display`, `Send`, `Sync`, `'static`.
- `From<io::Error>`, `From<toml::de::Error>`, `From<notify::Error>`, etc.
- Keep `anyhow::Context`-like API so call sites change minimally.

### Migration
- Replace `use anyhow::Context;` and `anyhow::Result` with `crate::error::{Context, Result}`.
- Update `main.rs` return type to `crate::Result<()>`.
- Add feature-gated `From` impls for remaining external error types.

### Performance target
- Smaller binary (fewer vtables), no runtime regression.
- Error path no worse than `anyhow`.

### Validation
- `cargo check`, `cargo test`, `cargo clippy` clean.

---

## Directive 4 — Replace `chrono` with `std::time`

### Why
`chrono` is only used for `Utc::now()` timestamps and RFC 3339 serialization.
`std::time::SystemTime` covers both needs since Rust 1.34 and is lighter.

### Interface
Add to `src/lib.rs` or `src/time.rs`:

```rust
pub type Timestamp = std::time::SystemTime;

pub fn now() -> Timestamp { std::time::SystemTime::now() }

pub fn to_rfc3339(t: Timestamp) -> String { ... }
```

### Migration
- Replace `chrono::Utc` imports with `crate::time::now`.
- For serialization, use a custom `serde` module or `humantime`-style RFC 3339
  formatting (`SystemTime` + `chrono`-free format).
- `last_modified` in `indexer.rs`: convert `std::fs::Metadata::modified()`
  directly to `SystemTime`.

### Performance target
- Faster `now()` calls (no timezone lookup).
- Smaller dependency tree.

### Validation
- Serialized JSON timestamps remain RFC 3339.
- All tests pass.

---

## Directive 5 — Replace `serde_json` with internal JSON builder

### Why
`serde_json` is only used in `web.rs` via `json!` for a handful of simple
endpoints. A tiny internal JSON type removes the dependency and avoids serde
overhead for small payloads.

### Interface
Create `src/json.rs`:

```rust
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn to_string(&self) -> String;
    pub fn object() -> Self;
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>);
}

macro_rules! json { ... }
```

### Migration
- Replace `serde_json::json!` in `web.rs` with `crate::json::json!`.
- Implement `axum::response::IntoResponse` for `Value` or return `Json(Value)`
  through a thin wrapper.

### Performance target
- Endpoint serialization ≥ 1.5× faster on dashboard payloads.

### Validation
- Dashboard JSON responses identical to baseline.
- Unit tests for `Value::to_string`.

---

## Directive 6 — Replace `clap` with manual CLI parser

### Why
`clap` is only used to parse three subcommands and one optional `--config`
flag. Manual parsing is a few dozen lines and removes a heavy derive-macro
 dependency.

### Interface
Create `src/cli.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub command: Command,
}

#[derive(Debug, Clone, Default)]
pub enum Command { Run, Index, Init { path: PathBuf } }

impl Args {
    pub fn parse() -> Result<Self>;
}
```

### Migration
- Replace `#[derive(Parser)]` struct and `Cli::parse()` in `main.rs` with
  `cli::Args::parse()`.
- Keep subcommand names and flag semantics identical.

### Performance target
- Faster startup (no clap builder).
- Smaller binary.

### Validation
- Manual test: `./target/release/fract --help`, `init`, `index`, `run`.

---

## Directive 7 — Replace `uuid` with deterministic IDs

### Why
Proposal IDs only need uniqueness within a process, not cryptographic
uniqueness. A simple atomic counter is faster and removes the `uuid` dep.

### Interface
Create `src/id.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next() -> String {
    format!("fract-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}
```

### Migration
- Replace `Uuid::new_v4().to_string()` in `src/queue.rs` with `crate::id::next()`.
- Remove `use uuid::Uuid;`.

### Performance target
- ID generation ~10× faster.

### Validation
- IDs are unique and deterministic in tests.

---

## Directive 8 — Replace `tempfile` with std temp directory

### Why
Only used once in `daemon.rs` to create a scratch directory. `std::env::temp_dir()`
plus a random name is sufficient and removes the dependency.

### Interface
Create `src/scratch.rs`:

```rust
pub fn temp_dir(prefix: &str) -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let name = format!("{}-{}-{}", prefix, std::process::id(), random_u64());
    let path = base.join(name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
```

### Migration
- Replace `tempfile::tempdir()` in `daemon.rs` with `scratch::temp_dir("fract")`.
- Keep manual cleanup logic already present.

### Performance target
- Negligible runtime difference; smaller dependency tree.

### Validation
- Scratch directories are created and cleaned up.

---

## Directive 9 — Replace `async-trait` with native async trait

### Why
Rust 1.75 stabilized async fn in traits. `RefactorEngine` in `src/refactor.rs`
can drop the `#[async_trait]` macro.

### Migration
- Remove `use async_trait::async_trait;`.
- Remove `#[async_trait]` attributes.
- Keep trait object usage: `Arc<dyn RefactorEngine>` requires `async fn`
  in trait to be object-safe; `-> impl Future` or boxed future may be needed.
  Use `fn refactor(&self, ctx: RefactorContext) -> impl std::future::Future<Output = Result<RefactorOutput>> + Send`
  or a boxed future for the trait object.

### Performance target
- Fewer macro-generated futures, faster compile.

### Validation
- `cargo check` clean; daemon and web still compile.

---

## Directive 10 — Replace `tower-http` with axum built-ins / manual layer

### Why
`tower-http` is only used for CORS and static file serving. Axum 0.8 has
`ServeDir` re-exported and CORS can be implemented with a tiny middleware or
removed if the dashboard is same-origin.

### Migration Options
1. If dashboard is served from same server, drop CORS entirely and use
   `axum::routing::get_service(serve_dir)` or inline a static handler.
2. Otherwise, implement a manual `tower::Layer` for permissive CORS.

Recommended: drop `CorsLayer` and implement a simple static-file handler that
falls back to `static/index.html`.

### Performance target
- One less dependency; no runtime regression.

### Validation
- Dashboard loads at `http://127.0.0.1:7345/`.
- API endpoints respond correctly.

---

## Directive 11 — Remove unused dependencies

The following crates are declared in `Cargo.toml` but have zero source
references:

- `hex`
- `thiserror`
- `sha2`
- `reqwest`
- `tokio-test`

### Migration
Remove them from `[dependencies]` and `[dev-dependencies]`.

### Validation
- `cargo check` and `cargo test` still pass.

---

## Integration & Benchmarking

1. Apply each replacement module.
2. Update call sites.
3. Remove replaced crates from `Cargo.toml`.
4. Run `cargo check`, `cargo clippy -- -D warnings`, `cargo test`.
5. Run criterion or `cargo bench` where applicable (scanner, walk, json).
6. Compare binary size with `cargo build --release` and `ls -lh target/release/fract`.

## Stop Conditions

- Do not replace `tokio`, `axum`, `notify`, `git2`, `serde`, or `toml` in this
  iteration; they are core-framework dependencies.
- Do not change the public API of `fract` library crates beyond what is
  required to swap error/timestamp/JSON types.
- All existing tests must pass; new tests must be added for replacement
  modules.
