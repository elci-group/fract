//! Head-to-head performance suite for the dependency-replacement modules.
//!
//! Each replacement is measured against a deliberately naive stdlib baseline
//! that implements the same observable behaviour. Results are written to
//! `target/bench-results.json` and consumed by `scripts/perf-check.sh` for
//! regression gating.
//!
//! Run with: `cargo bench --bench perf_suite`.

use fract::cli::Args;
use fract::json::{json, Value};
use fract::scanner::{JsTsScanner, PythonScanner, RustScanner};
use fract::time::{now, to_rfc3339};
use fract::walk::Walk;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let mut results = BenchResults::default();

    bench_walk(&mut results);
    bench_scanner(&mut results);
    bench_json(&mut results);
    bench_time(&mut results);
    bench_id(&mut results);
    bench_cli(&mut results);

    let out = output_path();
    if let Some(parent) = out.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&out, results.to_json()).expect("failed to write bench results");
    println!("wrote {}", out.display());
    println!("{}", results.to_json());
}

fn output_path() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--output" {
            if let Some(p) = args.next() {
                return PathBuf::from(p);
            }
        }
    }
    PathBuf::from("target/bench-results.json")
}

#[derive(Default)]
struct BenchResults {
    walk_new_files_per_sec: f64,
    walk_naive_files_per_sec: f64,
    scanner_new_ns_per_scan: f64,
    scanner_naive_ns_per_scan: f64,
    json_new_ns_per_payload: f64,
    json_naive_ns_per_payload: f64,
    time_ns_per_format: f64,
    id_ns_per_id: f64,
    cli_ns_per_parse: f64,
}

impl BenchResults {
    fn to_json(&self) -> String {
        let mut obj = Value::object();
        obj.insert("walk_new_files_per_sec", self.walk_new_files_per_sec);
        obj.insert("walk_naive_files_per_sec", self.walk_naive_files_per_sec);
        obj.insert(
            "walk_speedup",
            ratio(self.walk_new_files_per_sec, self.walk_naive_files_per_sec),
        );
        obj.insert("scanner_new_ns_per_scan", self.scanner_new_ns_per_scan);
        obj.insert("scanner_naive_ns_per_scan", self.scanner_naive_ns_per_scan);
        obj.insert(
            "scanner_speedup",
            ratio(self.scanner_naive_ns_per_scan, self.scanner_new_ns_per_scan),
        );
        obj.insert("json_new_ns_per_payload", self.json_new_ns_per_payload);
        obj.insert("json_naive_ns_per_payload", self.json_naive_ns_per_payload);
        obj.insert(
            "json_speedup",
            ratio(self.json_naive_ns_per_payload, self.json_new_ns_per_payload),
        );
        obj.insert("time_ns_per_format", self.time_ns_per_format);
        obj.insert("id_ns_per_id", self.id_ns_per_id);
        obj.insert("cli_ns_per_parse", self.cli_ns_per_parse);
        obj.to_string()
    }
}

fn ratio(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        0.0
    } else {
        a / b
    }
}

/// Minimum of `runs` measurements, each over `iters` iterations. The minimum
/// is the most stable estimator for an operation's attainable cost because it
/// is the least affected by scheduler or filesystem noise.
fn measure<F: FnMut()>(runs: usize, iters: usize, mut f: F) -> std::time::Duration {
    // Warmup to populate caches and settle the allocator.
    for _ in 0..iters {
        f();
    }
    let mut best = std::time::Duration::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..iters {
            f();
        }
        let elapsed = start.elapsed();
        if elapsed < best {
            best = elapsed;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// walk
// ---------------------------------------------------------------------------

fn bench_walk(results: &mut BenchResults) {
    let root = make_tree(400);
    let ignore = vec!["target".to_string()];

    const WALK_ITERS: usize = 40;
    let new_elapsed = measure(11, WALK_ITERS, || {
        let n = Walk::new(root.clone(), ignore.clone())
            .files()
            .filter(|r| r.is_ok())
            .count();
        black_box(n);
    });
    let new_per_iter = new_elapsed.as_secs_f64() / WALK_ITERS as f64;
    let files = Walk::new(root.clone(), ignore.clone())
        .files()
        .filter(|r| r.is_ok())
        .count() as f64;
    results.walk_new_files_per_sec = files / new_per_iter;

    let naive_elapsed = measure(11, WALK_ITERS, || {
        let n = naive_walk(&root, &ignore);
        black_box(n);
    });
    let naive_per_iter = naive_elapsed.as_secs_f64() / WALK_ITERS as f64;
    results.walk_naive_files_per_sec = files / naive_per_iter;

    let _ = fs::remove_dir_all(&root);
}

fn naive_walk(root: &Path, ignore: &[String]) -> usize {
    fn rec(dir: &Path, ignore: &[String], count: &mut usize) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy();
            if ignore.iter().any(|p| rel.starts_with(p.as_str())) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    rec(&path, ignore, count);
                } else if meta.is_file() {
                    *count += 1;
                }
            }
        }
    }
    let mut count = 0;
    rec(root, ignore, &mut count);
    count
}

fn make_tree(file_count: usize) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fract_perf_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for i in 0..file_count {
        let dir = if i % 10 == 0 {
            root.join("target")
        } else {
            root.join("src")
        };
        fs::write(dir.join(format!("file_{i}.rs")), sample_rust(80)).unwrap();
    }
    root
}

// ---------------------------------------------------------------------------
// scanner
// ---------------------------------------------------------------------------

fn bench_scanner(results: &mut BenchResults) {
    let rust = sample_rust(10_000);
    let python = sample_python(10_000);
    let jsts = sample_jsts(10_000);

    const SCAN_ITERS: usize = 400;
    let new = measure(11, SCAN_ITERS, || {
        black_box(RustScanner::count_functions(&rust));
        black_box(RustScanner::count_branches(&rust));
        black_box(RustScanner::count_public_items(&rust));
        black_box(PythonScanner::count_functions(&python));
        black_box(JsTsScanner::count_functions(&jsts));
    });
    results.scanner_new_ns_per_scan = new.as_nanos() as f64 / SCAN_ITERS as f64;

    let naive = measure(11, SCAN_ITERS, || {
        black_box(naive_count(&rust, &["pub fn ", "fn "]));
        black_box(naive_count(&python, &["def "]));
        black_box(naive_count(&jsts, &["function ", "const "]));
    });
    results.scanner_naive_ns_per_scan = naive.as_nanos() as f64 / SCAN_ITERS as f64;
}

fn naive_count(text: &str, needles: &[&str]) -> usize {
    let mut count = 0;
    for line in text.lines() {
        for needle in needles {
            count += line.matches(needle).count();
        }
    }
    count
}

// ---------------------------------------------------------------------------
// json
// ---------------------------------------------------------------------------

fn bench_json(results: &mut BenchResults) {
    let payload = build_payload();

    const JSON_ITERS: usize = 4_000;
    let new = measure(11, JSON_ITERS, || {
        black_box(payload.to_string());
    });
    results.json_new_ns_per_payload = new.as_nanos() as f64 / JSON_ITERS as f64;

    let naive = measure(11, JSON_ITERS, || {
        black_box(naive_json(&payload));
    });
    results.json_naive_ns_per_payload = naive.as_nanos() as f64 / JSON_ITERS as f64;
}

fn naive_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(naive_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(obj) => {
            let parts: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, naive_json(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

fn build_payload() -> Value {
    let modules: Vec<Value> = (0..100)
        .map(|i| {
            json!({
                "path": format!("/src/file{:04}.rs", i),
                "lines": i * 10,
                "functions": i,
                "health": "Healthy",
            })
        })
        .collect();
    json!({ "modules": modules, "status": "ok" })
}

// ---------------------------------------------------------------------------
// time
// ---------------------------------------------------------------------------

fn bench_time(results: &mut BenchResults) {
    let t = now();
    const TIME_ITERS: usize = 200_000;
    let elapsed = measure(11, TIME_ITERS, || {
        black_box(to_rfc3339(t));
    });
    results.time_ns_per_format = elapsed.as_nanos() as f64 / TIME_ITERS as f64;
}

// ---------------------------------------------------------------------------
// id
// ---------------------------------------------------------------------------

fn bench_id(results: &mut BenchResults) {
    const ID_ITERS: usize = 200_000;
    let elapsed = measure(11, ID_ITERS, || {
        black_box(fract::id::next());
    });
    results.id_ns_per_id = elapsed.as_nanos() as f64 / ID_ITERS as f64;
}

// ---------------------------------------------------------------------------
// cli
// ---------------------------------------------------------------------------

fn bench_cli(results: &mut BenchResults) {
    let args = [
        "fract",
        "--config",
        "fract.toml",
        "init",
        "--path",
        "/tmp/project",
    ];
    const CLI_ITERS: usize = 200_000;
    let elapsed = measure(11, CLI_ITERS, || {
        let parsed = Args::parse_from(args).unwrap();
        black_box(parsed);
    });
    results.cli_ns_per_parse = elapsed.as_nanos() as f64 / CLI_ITERS as f64;
}

// ---------------------------------------------------------------------------
// sample generators
// ---------------------------------------------------------------------------

fn sample_rust(lines: usize) -> String {
    let mut out = String::new();
    for i in 0..lines {
        let line = match i % 7 {
            0 => format!("pub fn func_{i}() {{}}\n"),
            1 => "use std::fs;\n".into(),
            2 => "if a && b { } else { }\n".into(),
            3 => "pub struct Foo;\n".into(),
            4 => "for x in 0..10 { loop {} }\n".into(),
            5 => "match v { }\n".into(),
            _ => "// comment\n".into(),
        };
        out.push_str(&line);
    }
    out
}

fn sample_python(lines: usize) -> String {
    let mut out = String::new();
    for i in 0..lines {
        let line = match i % 6 {
            0 => format!("def func_{i}():\n"),
            1 => "import os\n".into(),
            2 => "if a and b:\n".into(),
            3 => "for x in y:\n".into(),
            4 => "else:\n".into(),
            _ => "    pass\n".into(),
        };
        out.push_str(&line);
    }
    out
}

fn sample_jsts(lines: usize) -> String {
    let mut out = String::new();
    for i in 0..lines {
        let line = match i % 7 {
            0 => format!("export function func_{i}() {{}}\n"),
            1 => "import fs from 'fs';\n".into(),
            2 => "if (a && b) { }\n".into(),
            3 => "export const foo = 1;\n".into(),
            4 => "for (;;) { }\n".into(),
            5 => "while (c || d) { }\n".into(),
            _ => "// comment\n".into(),
        };
        out.push_str(&line);
    }
    out
}
