//! Enhanced CLI for production use with progress reporting and output formatting.
//!
//! Provides progress indicators, machine-readable output (JSON/YAML),
//! configuration file support, and graceful cancellation.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::error::Result;
use tracing::{info, error};

/// Output format for transformation results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text (default)
    Text,
    /// JSON format for machine consumption
    Json,
    /// YAML format for configuration
    Yaml,
    /// SARIF format for CI/CD integration
    Sarif,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Yaml => write!(f, "yaml"),
            OutputFormat::Sarif => write!(f, "sarif"),
        }
    }
}

/// Progress reporter for batch operations.
pub struct ProgressReporter {
    total: usize,
    current: usize,
    format: OutputFormat,
}

impl ProgressReporter {
    pub fn new(total: usize, format: OutputFormat) -> Self {
        Self {
            total,
            current: 0,
            format,
        }
    }

    /// Report progress on current operation.
    pub fn update(&mut self, current: usize, operation: &str) {
        self.current = current;
        match self.format {
            OutputFormat::Text => {
                let percent = (current as f64 / self.total as f64) * 100.0;
                println!("[{:.0}%] {}", percent, operation);
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "progress": current,
                    "total": self.total,
                    "percent": (current as f64 / self.total as f64) * 100.0,
                    "operation": operation
                });
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("progress: {}", current);
                println!("total: {}", self.total);
                println!("operation: {}", operation);
            }
            OutputFormat::Sarif => {
                // SARIF doesn't typically include progress
            }
        }
    }

    pub fn finish(&self, success: usize, failed: usize) {
        match self.format {
            OutputFormat::Text => {
                println!("Completed: {} succeeded, {} failed", success, failed);
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "status": "completed",
                    "succeeded": success,
                    "failed": failed,
                    "total": self.total
                });
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("status: completed");
                println!("succeeded: {}", success);
                println!("failed: {}", failed);
                println!("total: {}", self.total);
            }
            OutputFormat::Sarif => {
                // SARIF output with results
            }
        }
    }
}

/// Configuration file structure (parsed from ~/.fract.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShatterConfig {
    pub skip_validation: bool,
    pub dry_run: bool,
    pub output_format: String,
    pub progress: bool,
    pub parallel_jobs: usize,
    pub checkpoint_dir: Option<PathBuf>,
}

impl Default for ShatterConfig {
    fn default() -> Self {
        Self {
            skip_validation: false,
            dry_run: false,
            output_format: "text".to_string(),
            progress: true,
            parallel_jobs: 4,  // Default to 4 jobs instead of runtime detection
            checkpoint_dir: None,
        }
    }
}

impl ShatterConfig {
    /// Load configuration from file.
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: ShatterConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from default locations.
    pub fn load() -> Result<Self> {
        // Use a simpler approach without dirs crate
        match std::env::var("HOME") {
            Ok(home) => {
                let config_path = PathBuf::from(home).join(".fract.toml");
                info!(path = %config_path.display(), "Loading configuration from default location");
                Self::from_file(&config_path)
            }
            Err(e) => {
                error!(error = %e, "HOME environment variable not set, using default configuration");
                Ok(Self::default())
            }
        }
    }

    /// Save configuration to file.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Machine-readable transformation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformationResult {
    pub status: String,  // "success", "partial", "failed"
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub duration_ms: u128,
    pub files_modified: Vec<PathBuf>,
    pub errors: Vec<TransformationError>,
    pub metrics: ResultMetrics,
}

/// Individual transformation error for machine consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformationError {
    pub candidate: String,
    pub code: String,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Result metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMetrics {
    pub success_rate: f64,
    pub avg_confidence: f64,
    pub avg_duration_ms: f64,
}

impl TransformationResult {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn to_yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }
}

/// Cancellation token for graceful shutdown.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reporter_text_format() {
        let mut reporter = ProgressReporter::new(10, OutputFormat::Text);
        reporter.update(5, "processing candidates");
        assert_eq!(reporter.current, 5);
    }

    #[test]
    fn shatter_config_default() {
        let config = ShatterConfig::default();
        assert!(!config.skip_validation);
        assert!(config.progress);
    }

    #[test]
    fn transformation_result_serialization() {
        let result = TransformationResult {
            status: "success".to_string(),
            attempted: 10,
            succeeded: 9,
            failed: 1,
            duration_ms: 5000,
            files_modified: vec![],
            errors: vec![],
            metrics: ResultMetrics {
                success_rate: 0.9,
                avg_confidence: 0.85,
                avg_duration_ms: 500.0,
            },
        };

        assert!(result.to_json().is_ok());
    }

    #[test]
    fn cancellation_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }
}
