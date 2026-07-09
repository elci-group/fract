//! Performance smoke test for `fract::time`.
//!
//! Run with `cargo bench --bench chrono_bench`.

use fract::time::{now, to_rfc3339};
use std::time::Instant;

fn main() {
    let iterations = 1_000_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = now();
    }
    let now_elapsed = start.elapsed();
    println!("now() {} calls in {:?} ({:?} each)", iterations, now_elapsed, now_elapsed / iterations);

    let t = now();
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = to_rfc3339(t);
    }
    let fmt_elapsed = start.elapsed();
    println!("to_rfc3339() {} calls in {:?} ({:?} each)", iterations, fmt_elapsed, fmt_elapsed / iterations);
}
