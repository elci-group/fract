//! Performance smoke test for the internal JSON serializer.
//!
//! Run with `cargo bench --bench serde_json_bench` after adding the following
//! to `Cargo.toml`:
//!
//! ```toml
//! [[bench]]
//! name = "serde_json_bench"
//! harness = false
//! ```

use fract::json::{json, Value};
use std::time::Instant;

fn build_payload() -> Value {
    let modules: Vec<Value> = (0..100)
        .map(|i| {
            json!({
                "path": format!("/src/file{:04}.rs", i),
                "language": "rust",
                "lines": i * 10,
                "functions": i,
                "cyclomatic_complexity": i * 2,
                "health": "Healthy",
            })
        })
        .collect();

    let proposals: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("fract-{}", i),
                "confidence": 0.85,
                "status": "pending",
            })
        })
        .collect();

    json!({
        "modules": modules,
        "proposals": proposals,
        "config": {
            "mode": "daemon",
            "entropy_threshold": 0.75,
            "confidence_threshold": 0.6,
            "quiet_period_secs": 300,
        }
    })
}

fn main() {
    let payload = build_payload();
    let iterations = 10_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = payload.to_string();
    }
    let elapsed = start.elapsed();

    println!(
        "serialized {} payloads in {:?} ({:?} each, {} bytes)",
        iterations,
        elapsed,
        elapsed / iterations,
        payload.to_string().len()
    );
}
