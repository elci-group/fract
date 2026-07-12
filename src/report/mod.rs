//! Unified output model and renderers for fract.
//!
//! Every human- and machine-facing surface builds a [`Report`] from the same
//! source data and renders it through one formatter, so the CLI, HTTP API,
//! dashboard, and PR bodies all agree on schema, wording, and ordering. This
//! module is intentionally dependency-free: JSON is emitted through the in-tree
//! [`crate::json::Value`] and colours/widths are computed with `std` only.

mod human;
mod json_out;
mod markdown;
mod model;
mod sarif;
mod style;

pub use model::{suggest_kind, Finding, Report, Severity, Summary, SCHEMA};
pub use style::{ColorChoice, OutputFormat, Style, Verbosity};

pub(crate) use human::render_human;
pub(crate) use json_out::{render_json, render_jsonl};
pub(crate) use markdown::render_markdown;
pub(crate) use sarif::render_sarif;

// The `#[cfg(test)]` module below resolves `Module`, `Health`, `Path`, and
// `PathBuf` through `use super::*;` — exactly as it did against the original
// single-file `report.rs`. These names are not referenced in `mod.rs` itself,
// so the imports stay private (no change to the crate-visible surface) and the
// lint is silenced locally.
#[allow(unused_imports)]
use crate::{Health, Module};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now;

    fn module(path: &str, entropy: f64, lines: usize) -> Module {
        Module {
            path: PathBuf::from(path),
            language: crate::Language::Rust,
            lines,
            functions: lines / 5,
            cyclomatic_complexity: 1,
            public_api_size: 0,
            fan_out: 0,
            fan_in: 0,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: Some(0.9),
            churn: 0,
            test_coverage: 0.0,
            entropy,
            health: Health::from_entropy(entropy),
            last_modified: now(),
        }
    }

    #[test]
    fn ordering_is_entropy_desc_then_path() {
        let mods = vec![
            module("b.rs", 0.5, 10),
            module("a.rs", 0.9, 100),
            module("c.rs", 0.9, 100),
        ];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, false);
        let paths: Vec<_> = r
            .findings
            .iter()
            .map(|f| f.module.to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, vec!["a.rs", "c.rs", "b.rs"]);
    }

    #[test]
    fn over_only_drops_healthy() {
        let mods = vec![module("ok.rs", 0.3, 10), module("bad.rs", 0.95, 100)];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, true);
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].module, PathBuf::from("bad.rs"));
    }

    #[test]
    fn threshold_drives_severity_not_just_health_band() {
        // 0.60 is "Healthy" by the hard band but over a 0.40 threshold, so it
        // must surface as actionable (warning), with message/why agreeing.
        let mods = vec![module("mid.rs", 0.60, 100)];
        let r = Report::from_modules(Path::new("."), &mods, 0.40, false);
        assert_eq!(r.findings[0].severity, Severity::Warning);
        assert!(r.findings[0].why.contains("exceeds"));
        assert!(r.findings[0].message.contains("Over entropy threshold"));
    }

    #[test]
    fn human_contains_table_and_findings() {
        let mods = vec![module("big.rs", 0.95, 2000)];
        let r = Report::from_modules(Path::new("/proj"), &mods, 0.82, true);
        let out = render_human(
            &r,
            &Style {
                color: false,
                width: 100,
            },
            Verbosity::Normal,
        );
        assert!(out.contains("MODULE"));
        assert!(out.contains("big.rs"));
        assert!(out.contains("Findings"));
        assert!(out.contains("why"));
        assert!(!out.contains("\x1b["));
    }

    #[test]
    fn json_round_trips_schema() {
        let mods = vec![module("a.rs", 0.9, 100)];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, true);
        let v = render_json(&r);
        let text = v.to_string();
        assert!(text.contains("fract.report/v1"));
        assert!(text.contains("\"findings\""));
    }

    #[test]
    fn sarif_is_minimally_valid() {
        let mods = vec![module("a.rs", 0.9, 100), module("ok.rs", 0.1, 5)];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, true);
        let v = render_sarif(&r);
        let text = v.to_string();
        assert!(text.contains("\"version\":\"2.1.0\""));
        assert!(text.contains("\"runs\""));
        // Only the over-threshold module becomes a result.
        assert!(text.contains("a.rs"));
        assert!(!text.contains("ok.rs"));
    }

    #[test]
    fn markdown_has_table() {
        let mods = vec![module("a.rs", 0.9, 100)];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, true);
        let md = render_markdown(&r);
        assert!(md.contains("| Module | Entropy |"));
        assert!(md.contains("a.rs"));
    }

    #[test]
    fn color_choice_respects_no_color() {
        std::env::set_var("NO_COLOR", "1");
        let s = Style::detect(ColorChoice::Auto);
        assert!(!s.color);
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn verbosity_from_count_clamps() {
        assert_eq!(Verbosity::from_count(-5), Verbosity::Quiet);
        assert_eq!(Verbosity::from_count(0), Verbosity::Normal);
        assert_eq!(Verbosity::from_count(1), Verbosity::Verbose);
        assert_eq!(Verbosity::from_count(9), Verbosity::Debug);
    }

    #[test]
    fn markdown_golden_snapshot() {
        // Deterministic fixture: one critical, one excellent module.
        let mods = vec![module("big.rs", 0.95, 2000), module("ok.rs", 0.30, 10)];
        let r = Report::from_modules(Path::new("/proj"), &mods, 0.82, false);
        let md = render_markdown(&r);
        let expected = concat!(
            "## fract architectural health report\n",
            "\n",
            "**Root:** `/proj` · **Modules:** 2 · **Health:** 60%\n",
            "**Counts:** 1 excellent · 0 healthy · 0 warning · 1 critical\n",
            "\n",
            "| Module | Entropy | Severity | Next action |\n",
            "| --- | --- | --- | --- |\n",
            "| `big.rs` | 0.95 | critical | Split responsibilities into submodules |\n",
            "\n",
            "### `big.rs`\n",
            "- **Why:** entropy 0.95 exceeds threshold 0.82\n",
            "- **Next:** Split responsibilities into submodules\n",
            "- **Confidence:** 90%\n",
        );
        assert_eq!(md, expected);
    }

    #[test]
    fn unknown_confidence_renders_as_dash_in_human_and_null_in_json() {
        let mut m = module("mystery.rs", 0.9, 100);
        m.confidence = None;
        let r = Report::from_modules(Path::new("."), &[m], 0.82, true);
        let human = render_human(
            &r,
            &Style {
                color: false,
                width: 100,
            },
            Verbosity::Normal,
        );
        assert!(
            human.contains("—"),
            "human should show em dash for unknown confidence"
        );
        let json = render_json(&r).to_string();
        assert!(
            json.contains("\"confidence\":null"),
            "json should emit null confidence, got {json}"
        );
    }

    #[test]
    fn noise_budget_keeps_worst_and_preserves_summary() {
        let mods: Vec<Module> = (0..5)
            .map(|i| module(&format!("m{i}.rs"), 0.83 + f64::from(i) * 0.03, 100))
            .collect();
        let mut r = Report::from_modules(Path::new("."), &mods, 0.82, false);
        let total = r.summary.total;
        assert_eq!(total, 5);
        r.apply_budget(2);
        assert_eq!(r.summary.total, 5, "summary must reflect the whole project");
        assert_eq!(r.findings.len(), 2, "budget caps listed findings");
        assert!(
            r.findings[0].entropy >= r.findings[1].entropy,
            "highest-entropy findings are kept first"
        );
    }

    #[test]
    fn sarif_structural_invariants_hold() {
        use crate::json::{self, Value};

        fn field<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
            match v {
                Value::Object(o) => o.iter().find(|(kk, _)| kk == k).map(|(_, vv)| vv),
                _ => None,
            }
        }
        fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
            match field(v, k) {
                Some(Value::Array(items)) => items,
                _ => &[],
            }
        }
        fn as_str(v: &Value) -> Option<&str> {
            match v {
                Value::String(s) => Some(s),
                _ => None,
            }
        }

        let mods = vec![module("a.rs", 0.9, 100), module("b.rs", 0.85, 50)];
        let r = Report::from_modules(Path::new("."), &mods, 0.82, true);
        let v = json::parse(&render_sarif(&r).to_string()).unwrap();

        let runs = arr(&v, "runs");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        let tool = field(run, "tool").unwrap();
        let driver = field(tool, "driver").unwrap();
        let rules = arr(driver, "rules");
        let rule_ids: Vec<&str> = rules
            .iter()
            .filter_map(|r| field(r, "id").and_then(as_str))
            .collect();

        let results = arr(run, "results");
        assert_eq!(results.len(), 2);
        for res in results {
            let level = field(res, "level").and_then(as_str).unwrap();
            assert!(
                ["note", "warning", "error"].contains(&level),
                "bad level {level}"
            );
            let rid = field(res, "ruleId").and_then(as_str).unwrap();
            assert!(
                rule_ids.contains(&rid),
                "result references unknown rule {rid}"
            );
            let locs = arr(res, "locations");
            let phys = field(&locs[0], "physicalLocation").unwrap();
            let uri = field(field(phys, "artifactLocation").unwrap(), "uri").and_then(as_str);
            assert!(uri.is_some(), "every result needs an artifact uri");
        }
    }
}
