//! Full-project indexing: walks the tree with `walk`, dispatches each
//! supported file to its language scanner, and derives `Module` metrics
//! (fan-in from a call-graph approximation). `churn`, `test_coverage`,
//! and `edit_frequency` are stubbed to 0 pending git/coverage wiring.

use crate::error::{Context, Result};
use crate::scanner::{JsTsScanner, PythonScanner, RustScanner};
use crate::time::{now, Timestamp};
use crate::walk::Walk;
use crate::{Health, Language, Module};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Indexer {
    root: PathBuf,
    ignore: Vec<String>,
}

/// Result of a full-project index: the modules fract could actually analyze,
/// plus how many files the walk visited in total. The gap between the two
/// (files walked but not turned into a module — unsupported language, empty,
/// or unreadable) is fract's `excluded` count for `Summary.coverage`
/// (ELCI-DSEQ-EITR-001 §5/§7.1).
pub struct IndexOutcome {
    pub modules: Vec<Module>,
    pub files_walked: usize,
}

impl Indexer {
    /// Create an indexer rooted at `root` with ignore glob patterns.
    #[must_use]
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self {
        Self { root, ignore }
    }

    /// Index every supported file under the root.
    ///
    /// # Errors
    /// Returns an error if a directory entry cannot be read or a supported
    /// source file cannot be read or stat'ed.
    pub fn index(&self) -> Result<IndexOutcome> {
        let mut modules = Vec::new();
        let mut files_walked = 0usize;
        for entry in Walk::new(self.root.clone(), self.ignore.clone()).files() {
            let path = entry?;
            files_walked += 1;
            let lang = Language::from_path(&path);
            if lang == Language::Other {
                continue;
            }
            if let Some(module) = self.analyze_file(&path, lang)? {
                modules.push(module);
            }
        }

        // Compute fan-in from call graph approximation.
        let mut fan_in: HashMap<PathBuf, usize> = HashMap::new();
        for module in &modules {
            for dep in self.rough_imports(module) {
                *fan_in.entry(dep).or_insert(0) += 1;
            }
        }
        for module in &mut modules {
            module.fan_in = *fan_in.get(&module.path).unwrap_or(&0);
        }

        Ok(IndexOutcome {
            modules,
            files_walked,
        })
    }

    /// Match a relative path against a glob pattern.
    #[must_use]
    pub fn glob_match(path: &str, pat: &str) -> bool {
        crate::walk::glob_match(path, pat)
    }

    /// Re-index a single path (for incremental notify updates). Returns `None`
    /// for empty/unsupported files. `path` may be absolute (under `self.root`)
    /// or already relative.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or stat'ed.
    pub fn index_file(&self, path: &Path) -> Result<Option<Module>> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let lang = Language::from_path(&abs);
        if lang == Language::Other {
            return Ok(None);
        }
        if !abs.exists() {
            // Deleted between the notify event and now: treat as removal.
            return Ok(None);
        }
        self.analyze_file(&abs, lang)
    }

    fn analyze_file(&self, path: &Path, language: Language) -> Result<Option<Module>> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let lines = text.lines().count();
        if lines == 0 {
            return Ok(None);
        }

        let metadata =
            std::fs::metadata(path).with_context(|| format!("metadata for {}", path.display()))?;
        let last_modified: Timestamp = metadata.modified().unwrap_or_else(|_| now());

        let (functions, cyclomatic) = match language {
            Language::Rust => (
                RustScanner::count_functions(&text),
                RustScanner::count_branches(&text),
            ),
            Language::Python => (
                PythonScanner::count_functions(&text),
                PythonScanner::count_branches(&text),
            ),
            Language::TypeScript | Language::JavaScript => (
                JsTsScanner::count_functions(&text),
                JsTsScanner::count_branches(&text),
            ),
            Language::Other => (0, 0),
        };

        let public_api_size = count_public_api(&text, language);
        let fan_out = count_imports(&text, language);
        let duplicates = estimate_duplication(&text);
        let churn = 0; // Would integrate with git history.
        let test_coverage = 0.0; // Would integrate with coverage tooling.
        let edit_frequency = 0.0; // Would derive from event history.
        let confidence = None; // No AI confidence at index time; measured later per-proposal.

        let mut module = Module {
            path: path.strip_prefix(&self.root).unwrap_or(path).to_path_buf(),
            language,
            lines,
            functions,
            cyclomatic_complexity: cyclomatic,
            public_api_size,
            fan_out,
            fan_in: 0,
            duplicates,
            edit_frequency,
            confidence,
            churn,
            test_coverage,
            entropy: 0.0,
            health: Health::Healthy,
            last_modified,
        };

        module.entropy = crate::complexity::entropy(&module);
        module.health = Health::from_entropy(module.entropy);
        Ok(Some(module))
    }

    /// Very rough import extraction used only for fan-in approximation.
    fn rough_imports(&self, module: &Module) -> Vec<PathBuf> {
        let Ok(text) = std::fs::read_to_string(self.root.join(&module.path)) else {
            return Vec::new();
        };
        let mut deps = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if module.language == Language::Rust && line.starts_with("use ") {
                // Best-effort: ignore external crates.
                if let Some(rest) = line.strip_prefix("use crate::") {
                    let first = rest
                        .split("::")
                        .next()
                        .unwrap_or(rest)
                        .trim_end_matches(';');
                    deps.push(PathBuf::from(format!("src/{first}.rs")));
                }
            }
        }
        deps
    }
}

fn count_public_api(text: &str, language: Language) -> usize {
    match language {
        Language::Rust => RustScanner::count_public_items(text),
        Language::Python => PythonScanner::count_public_items(text),
        Language::TypeScript | Language::JavaScript => JsTsScanner::count_public_items(text),
        Language::Other => 0,
    }
}

fn count_imports(text: &str, language: Language) -> usize {
    match language {
        Language::Rust => RustScanner::count_imports(text),
        Language::Python => PythonScanner::count_imports(text),
        Language::TypeScript | Language::JavaScript => JsTsScanner::count_imports(text),
        Language::Other => 0,
    }
}

/// Minimum trimmed length for a line to count toward duplication; shorter
/// lines (braces, `else`, ...) are too common to carry a signal.
const DUPLICATE_LINE_MIN_LEN: usize = 16;

fn estimate_duplication(text: &str) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut dupes = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.len() >= DUPLICATE_LINE_MIN_LEN && !seen.insert(line.to_string()) {
            dupes += 1;
        }
    }
    dupes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn index_file_matches_full_index_for_one_module() {
        let root = crate_root();
        let indexer = Indexer::new(root.clone(), crate::config::default_ignore_patterns());
        let modules = indexer.index().expect("full index").modules;
        let target = modules
            .iter()
            .find(|m| m.path.as_path() == Path::new("src/model.rs"))
            .or_else(|| modules.first())
            .expect("at least one module")
            .clone();
        let full_path = root.join(&target.path);
        let single = indexer
            .index_file(&full_path)
            .expect("index_file ok")
            .expect("index_file some");
        assert_eq!(single.lines, target.lines);
        assert_eq!(single.functions, target.functions);
        assert!(
            (single.entropy - target.entropy).abs() < 1e-9,
            "entropy mismatch: {} vs {}",
            single.entropy,
            target.entropy
        );
    }

    #[test]
    fn index_file_returns_none_for_empty_and_other() {
        // Unique temp dir using only std (the `tempfile` crate is forbidden).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("fract-indexer-test-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let indexer = Indexer::new(dir.clone(), Vec::new());

        // Empty .rs file -> None (analyze_file skips empty files).
        let empty = dir.join("empty.rs");
        std::fs::write(&empty, b"").unwrap();
        assert!(indexer.index_file(&empty).unwrap().is_none());

        // `.txt` is unsupported -> None (short-circuits; path need not exist).
        let txt = dir.join("notes.txt");
        assert!(indexer.index_file(&txt).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-indexer-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn index_handles_python_typescript_and_unsupported_files() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.py"), "def f(x):\n    return x\n").unwrap();
        std::fs::write(
            dir.join("src/b.ts"),
            "export function g(x: number): number {\n    return x;\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/notes.txt"), "ignore me\n").unwrap();

        let indexer = Indexer::new(dir.clone(), Vec::new());
        let outcome = indexer.index().unwrap();
        assert_eq!(outcome.modules.len(), 2, "the .txt file must be skipped");
        assert_eq!(outcome.files_walked, 3, "all three files were visited");
        assert!(outcome.modules.iter().any(|m| m.language == Language::Python));
        assert!(outcome
            .modules
            .iter()
            .any(|m| m.language == Language::TypeScript));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_file_resolves_relative_paths_and_treats_missing_as_removal() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/c.rs"), "pub fn c() {}\n").unwrap();

        let indexer = Indexer::new(dir.clone(), Vec::new());
        // Relative paths resolve against the root.
        let module = indexer
            .index_file(Path::new("src/c.rs"))
            .unwrap()
            .expect("module");
        assert_eq!(module.path, PathBuf::from("src/c.rs"));
        // A supported file that vanished is treated as a removal.
        assert!(indexer
            .index_file(Path::new("src/gone.rs"))
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
