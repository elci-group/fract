//! Grounded prompt rendering and structured-output parsing for the LLM refactor
//! engine.
//!
//! The deterministic half of the LLM surface: [`render_prompt`] turns a
//! [`RefactorContext`] into a stable, self-contained instruction that *forces*
//! the model to reply with a single JSON object matching [`RefactorPlan`], and
//! [`parse_plan`] validates that reply strictly (including a grounding check
//! that no planned path escapes the project). A live provider only has to send
//! the prompt and hand the response text to `parse_plan`; everything that
//! determines output *quality* lives here and is fully tested offline.
//!
//! JSON is decoded with the in-tree [`crate::json::parse`], so this stays
//! dependency-free.

use crate::error::Result;
use crate::json::{self, as_array, as_object, as_str, get, get_str};
use crate::refactor::{RefactorContext, RefactorOutput};
use crate::DiffSummary;
use std::path::{Path, PathBuf};

/// The exact response shape the refactor engine must return.
#[derive(Debug, Clone, PartialEq)]
pub struct RefactorPlan {
    pub files: Vec<PlannedFile>,
    pub migration_notes: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub content: String,
}

/// Render the deterministic prompt for a refactor. Output is byte-stable for a
/// given context, which makes it golden-testable and cache-friendly.
#[must_use]
pub fn render_prompt(ctx: &RefactorContext) -> String {
    use std::fmt::Write;
    let m = &ctx.module;
    let mut out = String::new();

    let _ = writeln!(out, "You are fract, a conservative refactoring engine.");
    let _ = writeln!(out, "Refactor ONE module to reduce structural entropy while preserving observable behaviour and minimising public-API changes.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Respond with EXACTLY one JSON object and no other text, matching this schema:"
    );
    let _ = writeln!(out, "{{");
    let _ = writeln!(
        out,
        r#"  "rationale": "short explanation of the change and why it lowers entropy","#
    );
    let _ = writeln!(
        out,
        r#"  "migration_notes": ["human-readable note", "..."],"#
    );
    let _ = writeln!(
        out,
        r#"  "files": [{{"path": "relative/path.rs", "content": "full new file contents"}}]"#
    );
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "Rules: every `path` must be relative and stay inside the project; include the COMPLETE new content for each file; do not invent files outside this module.");
    let _ = writeln!(out);

    let _ = writeln!(out, "## Module");
    let _ = writeln!(out, "- path: {}", m.path.display());
    let _ = writeln!(out, "- language: {}", m.language);
    let _ = writeln!(out, "- lines: {}", m.lines);
    let _ = writeln!(out, "- functions: {}", m.functions);
    let _ = writeln!(out, "- cyclomatic_complexity: {}", m.cyclomatic_complexity);
    let _ = writeln!(out, "- public_api_size: {}", m.public_api_size);
    let _ = writeln!(out, "- fan_out: {} · fan_in: {}", m.fan_out, m.fan_in);
    let _ = writeln!(out, "- duplicates: {}", m.duplicates);
    let _ = writeln!(out, "- entropy: {:.4}", m.entropy);
    let _ = writeln!(out, "- health: {}", m.health);
    let _ = writeln!(out);

    write_list(&mut out, "Imports", &ctx.imports);
    write_list(&mut out, "Exports", &ctx.exports);

    let _ = writeln!(out, "## Project conventions");
    let _ = writeln!(out, "{}", ctx.project_conventions);
    let _ = writeln!(out);

    let _ = writeln!(out, "## Current source");
    let _ = writeln!(out, "{}", ctx.source);

    out
}

fn write_list(out: &mut String, title: &str, items: &[String]) {
    use std::fmt::Write;
    const CAP: usize = 60;
    let _ = writeln!(out, "## {title} ({} total)", items.len());
    for item in items.iter().take(CAP) {
        let _ = writeln!(out, "- {item}");
    }
    if items.len() > CAP {
        let _ = writeln!(out, "- … ({} more)", items.len() - CAP);
    }
    let _ = writeln!(out);
}

/// Parse and strictly validate a model response into a [`RefactorPlan`].
/// Accepts a bare JSON object or one wrapped in a triple-backtick `json` code
/// fence (optionally surrounded by prose), then enforces the schema and the
/// grounding rule that every planned path is relative and stays within the
/// project.
///
/// # Errors
/// Returns an error if the response is not valid JSON, is not an object, is
/// missing a non-empty `rationale` or a non-empty `files` array, any file
/// entry is malformed, a path fails the grounding check, or
/// `migration_notes` is present but not an array of strings.
pub fn parse_plan(input: &str) -> Result<RefactorPlan> {
    let text = extract_json(input);
    let value = json::parse(text).map_err(|e| format!("refactor plan is not valid JSON: {e}"))?;
    let obj = as_object(&value).ok_or_else(|| "refactor plan must be a JSON object".to_string())?;

    let rationale = get_str(obj, "rationale")
        .ok_or_else(|| "refactor plan missing string `rationale`".to_string())?;
    if rationale.trim().is_empty() {
        return Err("refactor plan `rationale` must not be empty".into());
    }

    let files_val = get(obj, "files").ok_or_else(|| "refactor plan missing `files`".to_string())?;
    let files_arr = as_array(files_val).ok_or_else(|| "`files` must be an array".to_string())?;
    if files_arr.is_empty() {
        return Err("refactor plan must contain at least one file".into());
    }

    let mut files = Vec::with_capacity(files_arr.len());
    for (i, item) in files_arr.iter().enumerate() {
        let fo = as_object(item).ok_or_else(|| format!("files[{i}] must be an object"))?;
        let path = get_str(fo, "path").ok_or_else(|| format!("files[{i}] missing `path`"))?;
        let content =
            get_str(fo, "content").ok_or_else(|| format!("files[{i}] missing `content`"))?;
        let path = check_grounded(path, i)?;
        files.push(PlannedFile {
            path,
            content: content.to_string(),
        });
    }

    let mut migration_notes = Vec::new();
    if let Some(notes) = get(obj, "migration_notes") {
        let arr =
            as_array(notes).ok_or_else(|| "`migration_notes` must be an array".to_string())?;
        for (i, n) in arr.iter().enumerate() {
            let s = as_str(n).ok_or_else(|| format!("migration_notes[{i}] must be a string"))?;
            migration_notes.push(s.to_string());
        }
    }

    Ok(RefactorPlan {
        files,
        migration_notes,
        rationale: rationale.to_string(),
    })
}

/// Convert a validated plan into a [`RefactorOutput`], computing a deterministic
/// [`DiffSummary`] by comparing the planned primary file against the original.
#[must_use]
pub fn plan_into_output(ctx: &RefactorContext, plan: RefactorPlan) -> RefactorOutput {
    let old_lines = ctx.source.lines().count();
    let mut files: Vec<(PathBuf, String)> = Vec::with_capacity(plan.files.len());
    let mut files_added = 0usize;
    let mut files_modified = 0usize;
    let mut primary_new_lines = old_lines;

    for pf in plan.files {
        if pf.path == ctx.module.path {
            files_modified += 1;
            primary_new_lines = pf.content.lines().count();
        } else {
            files_added += 1;
        }
        files.push((pf.path, pf.content));
    }

    let diff_summary = DiffSummary {
        files_added,
        files_removed: 0,
        files_modified,
        lines_added: primary_new_lines.saturating_sub(old_lines),
        lines_removed: old_lines.saturating_sub(primary_new_lines),
    };

    RefactorOutput {
        files,
        migration_notes: plan.migration_notes,
        diff_summary,
    }
}

fn check_grounded(path: &str, i: usize) -> Result<PathBuf> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err(format!("files[{i}] path must be relative, got absolute `{path}`").into());
    }
    if p.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(format!("files[{i}] path must not contain `..`: `{path}`").into());
    }
    if path.trim().is_empty() {
        return Err(format!("files[{i}] path must not be empty").into());
    }
    Ok(p.to_path_buf())
}

/// Pull the JSON object out of a model reply: prefer a triple-backtick fenced
/// block, else fall back to the substring spanning the first `{` .. last `}`.
fn extract_json(input: &str) -> &str {
    if let Some(start) = input.find("```") {
        let after = &input[start + 3..];
        // Skip an optional language tag on the opening fence line.
        let body = match after.find('\n') {
            Some(nl) => &after[nl + 1..],
            None => after,
        };
        if let Some(end) = body.find("```") {
            return body[..end].trim();
        }
    }
    match (input.find('{'), input.rfind('}')) {
        (Some(s), Some(e)) if e > s => input[s..=e].trim(),
        _ => input.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now;
    use crate::{Health, Language, Module};

    fn module() -> Module {
        Module {
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            lines: 10,
            functions: 4,
            cyclomatic_complexity: 6,
            public_api_size: 2,
            fan_out: 3,
            fan_in: 1,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: Some(0.9),
            churn: 0,
            test_coverage: 0.0,
            entropy: 0.91,
            health: Health::from_entropy(0.91),
            last_modified: now(),
        }
    }

    fn ctx() -> RefactorContext {
        RefactorContext {
            module: module(),
            source: "pub fn a() {}\npub fn b() {}\n".to_string(),
            imports: vec!["use std::io;".to_string()],
            exports: vec!["pub fn a() {}".to_string()],
            dependents: Vec::new(),
            project_conventions: "max_width = 100".to_string(),
        }
    }

    #[test]
    fn prompt_is_deterministic_and_grounded() {
        let c = ctx();
        let a = render_prompt(&c);
        let b = render_prompt(&c);
        assert_eq!(a, b, "prompt must be byte-stable");
        assert!(a.contains("src/lib.rs"));
        assert!(a.contains("entropy: 0.9100"));
        assert!(a.contains("Respond with EXACTLY one JSON object"));
        assert!(a.contains("pub fn a() {}"));
        assert!(a.contains("max_width = 100"));
    }

    const GOOD: &str = r#"{
        "rationale": "split public and private items to cut fan-out",
        "migration_notes": ["preserve behaviour", "keep public API"],
        "files": [
            {"path": "src/lib.rs", "content": "pub fn a() {}\n"},
            {"path": "src/internal.rs", "content": "fn b() {}\n"}
        ]
    }"#;

    #[test]
    fn parses_clean_json() {
        let plan = parse_plan(GOOD).unwrap();
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.migration_notes.len(), 2);
        assert!(plan.rationale.starts_with("split"));
    }

    #[test]
    fn parses_fenced_json_with_prose() {
        let wrapped = format!("Sure, here is the plan:\n```json\n{GOOD}\n```\nThanks!");
        let plan = parse_plan(&wrapped).unwrap();
        assert_eq!(plan.files.len(), 2);
    }

    #[test]
    fn rejects_missing_files() {
        let bad = r#"{"rationale":"x","files":[]}"#;
        assert!(parse_plan(bad).is_err());
    }

    #[test]
    fn rejects_missing_rationale() {
        let bad = r#"{"files":[{"path":"a.rs","content":""}]}"#;
        assert!(parse_plan(bad).is_err());
    }

    #[test]
    fn rejects_ungrounded_paths() {
        let abs = r#"{"rationale":"x","files":[{"path":"/etc/passwd","content":""}]}"#;
        assert!(parse_plan(abs).is_err());
        let dot = r#"{"rationale":"x","files":[{"path":"../escape.rs","content":""}]}"#;
        assert!(parse_plan(dot).is_err());
    }

    #[test]
    fn plan_into_output_computes_diff() {
        let c = ctx(); // source has 2 lines
        let plan = parse_plan(GOOD).unwrap();
        let out = plan_into_output(&c, plan);
        assert_eq!(out.files.len(), 2);
        assert_eq!(out.diff_summary.files_added, 1); // internal.rs
        assert_eq!(out.diff_summary.files_modified, 1); // lib.rs
    }
}
