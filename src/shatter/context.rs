//! Runtime state and configuration for shatter operations.

use std::path::PathBuf;
use crate::error::Result;

/// Configuration for a shatter operation.
#[derive(Debug, Clone)]
pub struct ShatterConfig {
    pub skip_validation: bool,
    pub dry_run: bool,
}

impl Default for ShatterConfig {
    fn default() -> Self {
        Self {
            skip_validation: false,
            dry_run: false,
        }
    }
}

/// Runtime context for a shatter transformation pipeline.
#[derive(Debug)]
pub struct ShatterContext {
    pub root: PathBuf,
    pub config: ShatterConfig,
}

impl ShatterContext {
    /// Create a new context for the given project root.
    pub fn new(root: PathBuf, config: ShatterConfig) -> Result<Self> {
        Ok(Self { root, config })
    }
}
