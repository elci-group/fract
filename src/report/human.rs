//! Human (terminal table) renderer.

use super::model::*;
use super::style::*;

// ---------------------------------------------------------------------------
// Human renderer
// ---------------------------------------------------------------------------

pub(crate) fn render_human(r: &Report, style: &Style, v: Verbosity) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let c = style.color;

    let head = format!(
        "{} · {} · {} modules · health {:.0}%",
        r.schema,
        r.root.display(),
        r.summary.total,
        r.summary.score
    );
    let _ = writeln!(out, "{}", paint(&head, "1", c));
    let _ = writeln!(
        out,
        "{} excellent · {} healthy · {} warning · {} critical",
        r.summary.excellent, r.summary.healthy, r.summary.warning, r.summary.critical
    );

    if v == Verbosity::Quiet {
        return out;
    }

    if r.findings.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "No modules over the entropy threshold.");
        return out;
    }

    let _ = writeln!(out);

    let headers = ["MODULE", "ENTROPY", "CONF", "SEVERITY", "NEXT ACTION"];
    let mut w_mod = headers[0].len();
    for f in &r.findings {
        w_mod = w_mod.max(display_width(&f.module.to_string_lossy()));
    }
    w_mod = w_mod.min(60);

    let _ = writeln!(
        out,
        "{:<w_mod$}  {:>7}  {:>4}  {:<8}  {}",
        headers[0], headers[1], headers[2], headers[3], headers[4]
    );
    let rule_len = (w_mod + 2 + 7 + 2 + 4 + 2 + 8 + 2 + 30).min(style.width);
    let _ = writeln!(out, "{}", "-".repeat(rule_len));

    for f in &r.findings {
        let path = ellipsize(&f.module.to_string_lossy(), w_mod);
        let sev = paint(
            &format!("{:<8}", f.severity.label()),
            severity_code(f.severity),
            c,
        );
        let _ = writeln!(
            out,
            "{:<w_mod$}  {:>7.2}  {:>4}  {}  {}",
            path,
            f.entropy,
            fmt_confidence_num(f.confidence),
            sev,
            ellipsize(&f.next_action, 40)
        );
    }

    let actionable: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::Warning)
        .collect();
    if !actionable.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", paint("Findings", "1", c));
        for f in actionable {
            let badge = paint(
                &format!("[{}]", f.severity.label()),
                severity_code(f.severity),
                c,
            );
            let _ = writeln!(out, "  {} {}", badge, f.module.display());
            let _ = writeln!(out, "      why : {}", f.why);
            let _ = writeln!(out, "      next: {}", f.next_action);
            if v >= Verbosity::Verbose {
                for (k, val) in &f.evidence {
                    let _ = writeln!(out, "      {k:<10}: {val}");
                }
            }
        }
    }

    out
}
