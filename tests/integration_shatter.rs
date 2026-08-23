//! Integration tests for fract shatter end-to-end transformations.

#[test]
fn test_shatter_cli_parse_candidates_file() {
    use std::path::PathBuf;
    use fract::cli::Args;

    let args = Args::parse_from([
        "fract",
        "shatter",
        "candidates.json",
        "--dry-run",
    ]).expect("parse args");

    match args.command {
        fract::cli::Command::Shatter {
            candidates,
            dry_run,
            skip_validation,
        } => {
            assert_eq!(candidates, PathBuf::from("candidates.json"));
            assert!(dry_run);
            assert!(!skip_validation);
        }
        _ => panic!("Expected Shatter command"),
    }
}

#[test]
fn test_shatter_loads_json_candidates() {
    use fract::shatter::load_candidates;
    use std::fs;
    use tempfile::TempDir;

    let json = r#"
[
  {
    "file": "src/lib.rs",
    "function": "helper",
    "target_module": "crate::utils",
    "target_file": "src/utils.rs",
    "confidence": 0.92
  }
]
"#;

    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("candidates.json");
    fs::write(&file_path, json).unwrap();

    let candidates = load_candidates(&file_path).expect("load candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].function_name, "helper");
    assert_eq!(candidates[0].confidence, 0.92);
}

#[test]
fn test_shatter_loads_toml_candidates() {
    use fract::shatter::load_candidates;
    use std::fs;
    use tempfile::TempDir;

    let toml = r#"
[[candidate]]
file = "src/lib.rs"
function = "process"
target_module = "crate::process"
target_file = "src/process.rs"
confidence = 0.88
"#;

    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("candidates.toml");
    fs::write(&file_path, toml).unwrap();

    let candidates = load_candidates(&file_path).expect("load candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].function_name, "process");
}

#[test]
fn test_shatter_validates_candidate_preconditions() {
    use fract::shatter::{CandidateFunction, validate_candidate};
    use std::path::PathBuf;

    let candidate = CandidateFunction::new(
        PathBuf::from("nonexistent.rs"),
        "foo".to_string(),
        "crate::utils".to_string(),
        PathBuf::from("src/utils.rs"),
        0.95,
    );

    // Should fail because file doesn't exist
    let result = validate_candidate(&candidate);
    assert!(result.is_err(), "Should reject candidate with nonexistent file");
}

#[test]
fn test_shatter_accepts_valid_function() {
    use fract::shatter::validate_candidate;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    let source = r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(source.as_bytes()).unwrap();
    file.flush().unwrap();

    use fract::shatter::CandidateFunction;
    let candidate = CandidateFunction::new(
        file.path().to_path_buf(),
        "add".to_string(),
        "crate::math".to_string(),
        PathBuf::from("src/math.rs"),
        0.95,
    );

    let result = validate_candidate(&candidate);
    assert!(result.is_ok(), "Should accept valid pure function");
}

#[test]
fn test_shatter_rejects_unsafe_function() {
    use fract::shatter::validate_candidate;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    let source = r#"
pub fn dangerous() {
    unsafe {
        std::ptr::null::<i32>();
    }
}
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(source.as_bytes()).unwrap();
    file.flush().unwrap();

    use fract::shatter::CandidateFunction;
    let candidate = CandidateFunction::new(
        file.path().to_path_buf(),
        "dangerous".to_string(),
        "crate::utils".to_string(),
        PathBuf::from("src/utils.rs"),
        0.95,
    );

    let result = validate_candidate(&candidate);
    assert!(result.is_err(), "Should reject function with unsafe block");
}

#[test]
fn test_shatter_rejects_panicking_function() {
    use fract::shatter::validate_candidate;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    let source = r#"
pub fn parse_number(s: &str) -> i32 {
    s.parse().unwrap()
}
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(source.as_bytes()).unwrap();
    file.flush().unwrap();

    use fract::shatter::CandidateFunction;
    let candidate = CandidateFunction::new(
        file.path().to_path_buf(),
        "parse_number".to_string(),
        "crate::utils".to_string(),
        PathBuf::from("src/utils.rs"),
        0.95,
    );

    let result = validate_candidate(&candidate);
    assert!(result.is_err(), "Should reject function with unwrap");
}

#[test]
fn test_shatter_move_enum_all_variants() {
    use fract::shatter::{Move, PublicVisibility};
    use std::path::PathBuf;

    let _extract = Move::ExtractFunction {
        source_file: PathBuf::from("src/lib.rs"),
        function_name: "foo".to_string(),
        target_module: "crate::utils".to_string(),
        target_file: PathBuf::from("src/utils.rs"),
        required_imports: vec![],
    };

    let _make_public = Move::MakePublic {
        file: PathBuf::from("src/lib.rs"),
        item_name: "helper".to_string(),
        visibility: PublicVisibility::Public,
    };

    let _add_import = Move::AddImport {
        file: PathBuf::from("src/lib.rs"),
        import_stmt: "use std::collections::HashMap;".to_string(),
    };

    let _remove_import = Move::RemoveImport {
        file: PathBuf::from("src/lib.rs"),
        import_path: "std::collections::HashMap".to_string(),
    };

    let _create_reexport = Move::CreateReExport {
        file: PathBuf::from("src/lib.rs"),
        item_name: "Helper".to_string(),
        original_module: "crate::utils".to_string(),
    };
}

#[test]
fn test_shatter_transaction_state_machine() {
    use fract::shatter::{Transaction, FileChange, transactional::TransactionState};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let mut tx = Transaction::new(dir.path().to_path_buf()).unwrap();

    // Initially pending
    assert_eq!(tx.state(), TransactionState::Pending);

    // Can stage changes
    let change = FileChange::new(
        dir.path().join("test.rs"),
        "original".to_string(),
        "modified".to_string(),
    );
    tx.stage(change).expect("stage");
    assert_eq!(tx.change_count(), 1);
}

#[test]
fn test_shatter_compiler_repair_analyzes_errors() {
    use fract::shatter::compiler_repair::{analyze_compiler_output, CompilationErrorKind};

    let output = "error[E0425]: cannot find function `helper` in this scope";
    let diag = analyze_compiler_output(output).unwrap();

    assert_eq!(diag.error_count(), 1);
    assert!(matches!(
        diag.errors[0].kind,
        CompilationErrorKind::UnresolvedReference(_)
    ));
}

#[test]
fn test_shatter_compiler_repair_suggests_fixes() {
    use fract::shatter::compiler_repair::{suggest_repair, CompilationError, CompilationErrorKind};
    use std::path::PathBuf;

    let error = CompilationError::new(
        CompilationErrorKind::PrivacyViolation("foo".to_string()),
        PathBuf::from("src/lib.rs"),
        "private item".to_string(),
    );

    let suggestions = suggest_repair(&error);
    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().any(|s| s.contains("public")));
}

#[test]
fn test_shatter_reanalysis_validates_post_transformation() {
    use fract::shatter::reanalysis::ReanalysisResult;

    let mut result = ReanalysisResult::new();
    assert!(result.is_valid);

    result.add_issue("problem".to_string(), "fix it".to_string());
    assert!(!result.is_valid);
    assert_eq!(result.issues.len(), 1);
    assert_eq!(result.suggestions.len(), 1);
}

#[test]
fn test_shatter_ast_rewriter_extracts_functions() {
    use fract::shatter::AstRewriter;
    use std::path::PathBuf;

    let source = r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#;

    let rewriter = AstRewriter::new(PathBuf::from("."));
    let parts = rewriter
        .extract_function_parts(source, "add")
        .expect("extract parts");

    assert!(parts.signature.contains("add"));
    assert!(parts.body.contains("a"));
    assert!(parts.body.contains("b"));
}

#[test]
fn test_shatter_ast_rewriter_removes_functions() {
    use fract::shatter::AstRewriter;
    use std::path::PathBuf;

    let source = r#"
pub fn foo() { println!("foo"); }
pub fn bar() { println!("bar"); }
"#;

    let rewriter = AstRewriter::new(PathBuf::from("."));
    let result = rewriter
        .remove_function(source, "foo")
        .expect("remove function");

    assert!(!result.contains("fn foo"));
    assert!(result.contains("fn bar"));
}

#[test]
fn test_shatter_ast_rewriter_adds_imports() {
    use fract::shatter::AstRewriter;
    use std::path::PathBuf;

    let source = r#"
fn main() {
    println!("hello");
}
"#;

    let rewriter = AstRewriter::new(PathBuf::from("."));
    let result = rewriter
        .add_import(source, "use std::collections::HashMap;")
        .expect("add import");

    assert!(result.contains("HashMap"));
}

#[test]
fn test_shatter_ast_rewriter_makes_public() {
    use fract::shatter::{AstRewriter, PublicVisibility};
    use std::path::PathBuf;

    let source = r#"fn add(a: i32, b: i32) -> i32 { a + b }"#;

    let rewriter = AstRewriter::new(PathBuf::from("."));
    let result = rewriter
        .make_public(source, "add", PublicVisibility::Public)
        .expect("make public");

    assert!(result.contains("pub fn add"));
}

#[test]
fn test_shatter_ast_rewriter_creates_reexport() {
    use fract::shatter::AstRewriter;
    use std::path::PathBuf;

    let source = r#"fn main() { }"#;

    let rewriter = AstRewriter::new(PathBuf::from("."));
    let result = rewriter
        .create_reexport(source, "Helper", "crate::utils")
        .expect("create reexport");

    assert!(result.contains("pub use crate::utils::Helper"));
}

#[test]
fn test_shatter_dependency_graph_construction() {
    use fract::shatter::DependencyGraph;
    use tempfile::TempDir;
    use std::fs;

    let dir = TempDir::new().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    // Create a simple Rust file
    let lib_rs = src_dir.join("lib.rs");
    fs::write(
        &lib_rs,
        r#"
pub fn helper() -> i32 { 42 }
pub fn wrapper() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();

    // Build graph
    let graph = DependencyGraph::build(dir.path()).expect("build graph");
    assert!(!graph.nodes.is_empty(), "Graph should have nodes");
}

#[test]
fn test_shatter_move_sequence_validation() {
    use fract::shatter::{Move, MoveSequence};
    use std::path::PathBuf;

    let moves = vec![
        Move::ExtractFunction {
            source_file: PathBuf::from("src/lib.rs"),
            function_name: "foo".to_string(),
            target_module: "crate::utils".to_string(),
            target_file: PathBuf::from("src/utils.rs"),
            required_imports: vec![],
        },
        Move::AddImport {
            file: PathBuf::from("src/utils.rs"),
            import_stmt: "use std::collections::HashMap;".to_string(),
        },
    ];

    let sequence = MoveSequence::new(moves, false);
    assert!(sequence.validate().is_ok(), "Valid move sequence should pass");
}
