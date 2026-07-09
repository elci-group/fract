//! Performance smoke test for `fract::walk` and `fract::scanner`.
//!
//! Run with `cargo bench --bench walkdir_scanner_bench`.

use fract::scanner::{JsTsScanner, PythonScanner, RustScanner};
use fract::walk::Walk;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let root = make_tree(100);

    let start = Instant::now();
    let count = Walk::new(root.clone(), vec!["target".into()])
        .files()
        .filter(|r| r.as_ref().map(|p| p.extension().and_then(|e| e.to_str()) == Some("rs")).unwrap_or(true))
        .count();
    let walk_time = start.elapsed();
    println!("walk {} rust files in {:?}", count, walk_time);

    let rust_sample = sample_rust(10_000);
    let python_sample = sample_python(10_000);
    let jsts_sample = sample_jsts(10_000);
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        RustScanner::count_functions(&rust_sample);
        RustScanner::count_branches(&rust_sample);
        RustScanner::count_public_items(&rust_sample);
        RustScanner::count_imports(&rust_sample);
    }
    let rust_time = start.elapsed();
    println!("rust scanner {:?} for {iterations} iterations", rust_time);

    let start = Instant::now();
    for _ in 0..iterations {
        PythonScanner::count_functions(&python_sample);
        PythonScanner::count_branches(&python_sample);
        PythonScanner::count_public_items(&python_sample);
        PythonScanner::count_imports(&python_sample);
    }
    let python_time = start.elapsed();
    println!("python scanner {:?} for {iterations} iterations", python_time);

    let start = Instant::now();
    for _ in 0..iterations {
        JsTsScanner::count_functions(&jsts_sample);
        JsTsScanner::count_branches(&jsts_sample);
        JsTsScanner::count_public_items(&jsts_sample);
        JsTsScanner::count_imports(&jsts_sample);
    }
    let jsts_time = start.elapsed();
    println!("js/ts scanner {:?} for {iterations} iterations", jsts_time);

    let _ = fs::remove_dir_all(&root);
}

fn make_tree(file_count: usize) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fract_walk_bench_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();

    for i in 0..file_count {
        let dir = if i % 10 == 0 {
            root.join("target")
        } else {
            root.join("src")
        };
        fs::write(dir.join(format!("file_{i}.rs")), sample_rust(100)).unwrap();
    }
    root
}

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
