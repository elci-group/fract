use std::time::Instant;

/// Performance smoke test for the uuid / tempfile replacements.
///
/// Run with `cargo run --bench uuid-tempfile-async-trait-tower-http` after
/// adding the following to `Cargo.toml`:
///
/// [[bench]]
/// name = "uuid-tempfile-async-trait-tower-http"
/// harness = false
fn main() {
    let count = 100_000;

    let start = Instant::now();
    for _ in 0..count {
        let _ = fract::id::next();
    }
    let id_elapsed = start.elapsed();
    println!(
        "generated {} ids in {:?} ({:?} each)",
        count,
        id_elapsed,
        id_elapsed / count
    );

    let start = Instant::now();
    let mut dirs = Vec::with_capacity(100);
    for _ in 0..100 {
        let path = fract::scratch::temp_dir("fract_bench").unwrap();
        dirs.push(path);
    }
    let scratch_elapsed = start.elapsed();
    for dir in dirs {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    println!(
        "created 100 scratch dirs in {:?} ({:?} each)",
        scratch_elapsed,
        scratch_elapsed / 100
    );
}
