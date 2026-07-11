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
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        cfg.project_root = cfg.project_root.canonicalize()?;
        Ok(cfg)
    }

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

fn default_entropy_threshold() -> f64 {
    0.82
}
fn default_confidence_threshold() -> f64 {
    0.90
}
fn default_quiet_period_secs() -> u64 {
    120
}
fn default_bind() -> String {
    "127.0.0.1:7345".to_string()
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
    32768
}

fn default_watch_patterns() -> Vec<String> {
    vec![
        "src/**/*.rs".to_string(),
        "**/*.py".to_string(),
        "**/*.ts".to_string(),
        "**/*.js".to_string(),
    ]
}

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
