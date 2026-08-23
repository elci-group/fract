//! Loading and parsing candidate transformation specifications.
//!
//! Supports JSON and TOML formats for defining extraction candidates.

use std::path::{Path, PathBuf};
use crate::error::{Context, Result};
use super::preconditions::CandidateFunction;

/// Load candidates from a JSON or TOML file.
pub fn load_candidates(path: &Path) -> Result<Vec<CandidateFunction>> {
    let content = std::fs::read_to_string(path)
        .context("reading candidates file")?;

    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "candidates file must have .json or .toml extension".to_string())?;

    match extension {
        "json" => parse_json_candidates(&content),
        "toml" => parse_toml_candidates(&content),
        ext => Err(format!("unsupported candidates format: {}", ext).into()),
    }
}

/// Parse JSON candidates format.
///
/// Expected format:
/// ```json
/// [
///   {
///     "file": "src/lib.rs",
///     "function": "helper_func",
///     "target_module": "crate::utils",
///     "target_file": "src/utils.rs",
///     "confidence": 0.92
///   }
/// ]
/// ```
fn parse_json_candidates(content: &str) -> Result<Vec<CandidateFunction>> {
    let parsed: serde_json::Value = serde_json::from_str(content)
        .context("parsing JSON candidates")?;

    let candidates_array = parsed.as_array()
        .ok_or_else(|| "candidates JSON must be an array".to_string())?;

    let mut candidates = Vec::new();

    for (idx, item) in candidates_array.iter().enumerate() {
        let obj = item.as_object()
            .ok_or_else(|| format!("candidate {} must be an object", idx))?;

        let file = obj.get("file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| format!("candidate {} missing 'file'", idx))?;

        let function_name = obj.get("function")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("candidate {} missing 'function'", idx))?;

        let target_module = obj.get("target_module")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("candidate {} missing 'target_module'", idx))?;

        let target_file = obj.get("target_file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| format!("candidate {} missing 'target_file'", idx))?;

        let confidence = obj.get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        candidates.push(CandidateFunction::new(
            file,
            function_name,
            target_module,
            target_file,
            confidence,
        ));
    }

    Ok(candidates)
}

/// Parse TOML candidates format.
///
/// Expected format:
/// ```toml
/// [[candidate]]
/// file = "src/lib.rs"
/// function = "helper_func"
/// target_module = "crate::utils"
/// target_file = "src/utils.rs"
/// confidence = 0.92
/// ```
fn parse_toml_candidates(content: &str) -> Result<Vec<CandidateFunction>> {
    let parsed: toml::Value = toml::from_str(content)
        .context("parsing TOML candidates")?;

    let candidates_array = parsed.get("candidate")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "TOML must have [[candidate]] array".to_string())?;

    let mut candidates = Vec::new();

    for (idx, item) in candidates_array.iter().enumerate() {
        let obj = item.as_table()
            .ok_or_else(|| format!("candidate {} must be a table", idx))?;

        let file = obj.get("file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| format!("candidate {} missing 'file'", idx))?;

        let function_name = obj.get("function")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("candidate {} missing 'function'", idx))?;

        let target_module = obj.get("target_module")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("candidate {} missing 'target_module'", idx))?;

        let target_file = obj.get("target_file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| format!("candidate {} missing 'target_file'", idx))?;

        let confidence = obj.get("confidence")
            .and_then(|v| v.as_float())
            .unwrap_or(1.0);

        candidates.push(CandidateFunction::new(
            file,
            function_name,
            target_module,
            target_file,
            confidence,
        ));
    }

    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_json() {
        let json = r#"
[
  {
    "file": "src/lib.rs",
    "function": "foo",
    "target_module": "crate::utils",
    "target_file": "src/utils.rs",
    "confidence": 0.95
  }
]
"#;
        let candidates = parse_json_candidates(json).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].function_name, "foo");
        assert_eq!(candidates[0].confidence, 0.95);
    }

    #[test]
    fn parse_json_requires_array() {
        let json = r#"{ "file": "src/lib.rs" }"#;
        assert!(parse_json_candidates(json).is_err());
    }

    #[test]
    fn parse_simple_toml() {
        let toml = r#"
[[candidate]]
file = "src/lib.rs"
function = "bar"
target_module = "crate::math"
target_file = "src/math.rs"
confidence = 0.88
"#;
        let candidates = parse_toml_candidates(toml).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].function_name, "bar");
        assert_eq!(candidates[0].confidence, 0.88);
    }

    #[test]
    fn toml_candidate_defaults_confidence() {
        let toml = r#"
[[candidate]]
file = "src/lib.rs"
function = "baz"
target_module = "crate::utils"
target_file = "src/utils.rs"
"#;
        let candidates = parse_toml_candidates(toml).unwrap();
        assert_eq!(candidates[0].confidence, 1.0);
    }
}
