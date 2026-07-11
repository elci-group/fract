//! Markdown (PR / comment friendly) renderer.

use super::model::*;
use super::style::fmt_confidence_pct;

// ---------------------------------------------------------------------------
// Markdown (PR / comment friendly)
// ---------------------------------------------------------------------------

pub(crate) fn render_markdown(r: &Report) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "## fract architectural health report");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "**Root:** `{}` · **Modules:** {} · **Health:** {:.0}%",
        r.root.display(),
        r.summary.total,
        r.summary.score
    );
    let _ = writeln!(
        out,
        "**Counts:** {} excellent · {} healthy · {} warning · {} critical",
        r.summary.excellent, r.summary.healthy, r.summary.warning, r.summary.critical
    );
    let actionable: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::Warning)
        .collect();
    if actionable.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "No modules over the entropy threshold.");
        return out;
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "| Module | Entropy | Severity | Next action |");
    let _ = writeln!(out, "| --- | --- | --- | --- |");
    for f in &actionable {
        let _ = writeln!(
            out,
            "| `{}` | {:.2} | {} | {} |",
            f.module.display(),
            f.entropy,
            f.severity.label(),
            f.next_action
        );
    }
    let _ = writeln!(out);
    for f in &actionable {
        let _ = writeln!(out, "### `{}`", f.module.display());
        let _ = writeln!(out, "- **Why:** {}", f.why);
        let _ = writeln!(out, "- **Next:** {}", f.next_action);
        let _ = writeln!(
            out,
            "- **Confidence:** {}",
            fmt_confidence_pct(f.confidence)
        );
    }
    out
}
