//! Performance smoke test for `fract::error`.
//!
//! Run with `cargo bench --bench anyhow_bench`.

use fract::error::Context;
use std::time::Instant;

fn main() {
    let iterations = 100_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let err: fract::error::Error = "baseline error".into();
        let _ = err.to_string();
    }
    let create = start.elapsed();

    let start = Instant::now();
    for i in 0..iterations {
        let result: Result<i32, std::io::Error> =
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        let err = result.with_context(|| format!("context {i}")).unwrap_err();
        let _ = err.to_string();
    }
    let context = start.elapsed();

    println!("error creation : {:?} for {iterations} iterations", create);
    println!("context wrap   : {:?} for {iterations} iterations", context);
}
