//! Performance smoke test for the manual CLI parser.
//!
//! Run with `cargo bench --bench clap_bench` once the target is registered
//! in Cargo.toml with `harness = false`.

use fract::cli::Args;
use std::time::Instant;

fn main() {
    let args = [
        "fract",
        "--config",
        "fract.toml",
        "init",
        "--path",
        "/tmp/project",
    ];
    let iterations = 100_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let parsed = Args::parse_from(args).unwrap();
        std::hint::black_box(parsed);
    }
    let elapsed = start.elapsed();

    println!(
        "parsed {} argument sets in {:?} ({:?} each)",
        iterations,
        elapsed,
        elapsed / iterations
    );
}
