//! Daemon configuration: a TOML-loaded `Config` with serde defaults for
//! thresholds, watch/ignore patterns, dashboard bind, output, and LLM
//! knobs. `project_root` is canonicalized on load.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Root of the project to watch.
    pub project_root: PathBuf,

    /// Mode of operation.
    #[serde(default)]
    pub mode: Mode,

    /// Entropy threshold above which a module enters the refactor queue.
    #[serde(default = "default_entropy_threshold")]
    pub entropy_threshold: f64,

    /// Minimum confidence required before auto-merging a proposal.
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,

    /// Seconds of no edits before a proposal is considered safe to merge.
    #[serde(default = "default_quiet_period_secs")]
    pub quiet_period_secs: u64,

    /// Watch patterns (gitignore-style globs).
    #[serde(default = "default_watch_patterns")]
    pub watch_patterns: Vec<String>,

    /// Ignore patterns.
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,

    /// Dashboard bind address.
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Output / observability knobs.
    #[serde(default)]
    pub output: OutputConfig,

    /// LLM refactor engine configuration.
    #[serde(default)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Passive,
    Assisted,
    Autonomous,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Mode::Passive => "passive",
            Mode::Assisted => "assisted",
            Mode::Autonomous => "autonomous",
        };
        f.write_str(s)
    }
}

/// Output and observability configuration. CLI flags override these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Default render format: human | json | jsonl | sarif | markdown.
    #[serde(default = "default_output_format")]
    pub format: String,
    /// Colour mode: auto | always | never.
    #[serde(default = "default_output_color")]
    pub color: String,
    /// Verbosity: quiet | normal | verbose | debug.
    #[serde(default = "default_output_verbosity")]
    pub verbosity: String,
    /// Daemon log format: pretty | compact.
    #[serde(default = "default_log_format")]
    pub log_format: String,
    /// Noise budget: maximum actionable findings to surface (0 = unlimited).
    #[serde(default)]
    pub max_findings: usize,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: default_output_format(),
            color: default_output_color(),
            verbosity: default_output_verbosity(),
            log_format: default_log_format(),
            max_findings: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider: "openai", "anthropic", "local", or "mock".
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    /// Model name.
    #[serde(default = "default_llm_model")]
    pub model: String,
    /// API endpoint (for local / self-hosted models).
    pub endpoint: Option<String>,
    /// API key.
    pub api_key: Option<String>,
    /// Maximum tokens per refactor request.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

impl Config {
    /// Load and parse a configuration file.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read, the TOML is invalid, or
    /// the configured `project_root` cannot be canonicalized.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        cfg.project_root = cfg.project_root.canonicalize()?;
        Ok(cfg)
    }

    /// Build the default configuration for a project root.
    #[must_use]
    pub fn default_for(root: PathBuf) -> Self {
        Self {
            project_root: root,
            mode: Mode::Passive,
            entropy_threshold: default_entropy_threshold(),
            confidence_threshold: default_confidence_threshold(),
            quiet_period_secs: default_quiet_period_secs(),
            watch_patterns: default_watch_patterns(),
            ignore_patterns: default_ignore_patterns(),
            bind: default_bind(),
            output: OutputConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

/// Default entropy at or above which a module becomes a refactor candidate.
const DEFAULT_ENTROPY_THRESHOLD: f64 = 0.82;
/// Default minimum confidence for a proposal to be accepted.
const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.9;
/// Default token budget for a single engine completion.
const DEFAULT_MAX_TOKENS: usize = 32768;
/// Default dashboard/API bind address (loopback only).
const DEFAULT_BIND: &str = "127.0.0.1:7345";

fn default_entropy_threshold() -> f64 {
    DEFAULT_ENTROPY_THRESHOLD
}
fn default_confidence_threshold() -> f64 {
    DEFAULT_CONFIDENCE_THRESHOLD
}
fn default_quiet_period_secs() -> u64 {
    120
}
fn default_bind() -> String {
    DEFAULT_BIND.to_string()
}
fn default_output_format() -> String {
    "human".to_string()
}
fn default_output_color() -> String {
    "auto".to_string()
}
fn default_output_verbosity() -> String {
    "normal".to_string()
}
fn default_log_format() -> String {
    "pretty".to_string()
}
fn default_llm_provider() -> String {
    "mock".to_string()
}
fn default_llm_model() -> String {
    "gpt-oss-120b".to_string()
}
fn default_max_tokens() -> usize {
    DEFAULT_MAX_TOKENS
}

fn default_watch_patterns() -> Vec<String> {
    vec![
        "src/**/*.rs".to_string(),
        "**/*.py".to_string(),
        "**/*.ts".to_string(),
        "**/*.js".to_string(),
    ]
}

/// The ignore patterns applied by default when none are configured.
#[must_use]
pub fn default_ignore_patterns() -> Vec<String> {
    vec![
        "target/**".to_string(),
        ".git/**".to_string(),
        ".fract/**".to_string(),
        "node_modules/**".to_string(),
        ".venv/**".to_string(),
        "dist/**".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-config-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_config(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("fract.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_reads_toml_and_canonicalizes_root() {
        let dir = temp_dir();
        let body = format!(
            "project_root = \"{}\"\nmode = \"assisted\"\nentropy_threshold = 0.5\nquiet_period_secs = 30\n",
            dir.display()
        );
        let path = write_config(&dir, &body);
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.project_root, dir.canonicalize().unwrap());
        assert_eq!(cfg.mode, Mode::Assisted);
        assert!((cfg.entropy_threshold - 0.5).abs() < f64::EPSILON);
        assert_eq!(cfg.quiet_period_secs, 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_applies_defaults_for_missing_fields() {
        let dir = temp_dir();
        let body = format!("project_root = \"{}\"\n", dir.display());
        let path = write_config(&dir, &body);
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.mode, Mode::Passive);
        assert!((cfg.entropy_threshold - 0.82).abs() < f64::EPSILON);
        assert!((cfg.confidence_threshold - 0.90).abs() < f64::EPSILON);
        assert_eq!(cfg.quiet_period_secs, 120);
        assert_eq!(cfg.bind, "127.0.0.1:7345");
        assert_eq!(cfg.watch_patterns.len(), 4);
        assert_eq!(cfg.ignore_patterns, default_ignore_patterns());
        assert_eq!(cfg.output.format, "human");
        assert_eq!(cfg.output.color, "auto");
        assert_eq!(cfg.output.verbosity, "normal");
        assert_eq!(cfg.output.log_format, "pretty");
        assert_eq!(cfg.output.max_findings, 0);
        // With no `[llm]` table at all, serde falls back to `LlmConfig`'s
        // derived `Default` (empty strings), not the field-level defaults;
        // `build_engine` treats an empty provider as "mock".
        assert!(cfg.llm.provider.is_empty());
        assert!(cfg.llm.model.is_empty());
        assert_eq!(cfg.llm.max_tokens, 0);
        assert!(cfg.llm.endpoint.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_applies_llm_field_defaults_when_table_present() {
        let dir = temp_dir();
        let body = format!("project_root = \"{}\"\n[llm]\n", dir.display());
        let path = write_config(&dir, &body);
        let cfg = Config::load(&path).unwrap();
        // Field-level `serde(default = ...)` kicks in once the table exists.
        assert_eq!(cfg.llm.provider, "mock");
        assert_eq!(cfg.llm.model, "gpt-oss-120b");
        assert_eq!(cfg.llm.max_tokens, 32_768);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_rejects_malformed_toml() {
        let dir = temp_dir();
        let path = write_config(&dir, "project_root = \n");
        assert!(Config::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_errors() {
        let dir = temp_dir();
        assert!(Config::load(dir.join("does-not-exist.toml")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_rejects_uncanonicalizable_root() {
        let dir = temp_dir();
        let path = write_config(&dir, "project_root = \"/no/such/dir/fract-xyz-123\"\n");
        assert!(Config::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_for_populates_all_fields() {
        let cfg = Config::default_for(PathBuf::from("/tmp/example"));
        assert_eq!(cfg.project_root, PathBuf::from("/tmp/example"));
        assert_eq!(cfg.mode, Mode::Passive);
        assert!((cfg.entropy_threshold - 0.82).abs() < f64::EPSILON);
        assert!((cfg.confidence_threshold - 0.90).abs() < f64::EPSILON);
        assert_eq!(cfg.quiet_period_secs, 120);
        assert_eq!(cfg.bind, "127.0.0.1:7345");
        assert_eq!(cfg.watch_patterns.len(), 4);
        // `default_for` uses the derived `LlmConfig::default` (empty provider;
        // `build_engine` maps that to the mock engine).
        assert!(cfg.llm.provider.is_empty());
    }

    #[test]
    fn mode_display_labels() {
        assert_eq!(Mode::Passive.to_string(), "passive");
        assert_eq!(Mode::Assisted.to_string(), "assisted");
        assert_eq!(Mode::Autonomous.to_string(), "autonomous");
    }
}
