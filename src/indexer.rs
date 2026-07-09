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

impl Indexer {
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self {
        Self { root, ignore }
    }

    pub fn index(&self) -> Result<Vec<Module>> {
        let mut modules = Vec::new();
        for entry in Walk::new(self.root.clone(), self.ignore.clone()).files() {
            let path = entry?;
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

        Ok(modules)
    }

    pub fn glob_match(path: &str, pat: &str) -> bool {
        crate::walk::glob_match(path, pat)
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
        let confidence = 0.5; // Placeholder for AI-generated confidence.

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
                    deps.push(PathBuf::from(format!("src/{}.rs", first)));
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

fn estimate_duplication(text: &str) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut dupes = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.len() >= 16 && !seen.insert(line.to_string()) {
            dupes += 1;
        }
    }
    dupes
}
