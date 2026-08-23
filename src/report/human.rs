//! Human (terminal table) renderer.

use super::model::{Finding, Report, Severity};
use super::style::{
    display_width, ellipsize, fmt_confidence_num, glass_border, glass_paint, gradient_paint,
    severity_code, Style, Verbosity, ACCENT_COLOR, GLASS_BASE, GLASS_HIGHLIGHT, HEADER_COLOR,
    SUBTLE_COLOR, SUCCESS_COLOR,
};

// ---------------------------------------------------------------------------
// Human renderer
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
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
    let _ = writeln!(out, "{}", gradient_paint(&head, c));
    let _ = writeln!(
        out,
        "{} {} excellent · {} healthy · {} warning · {} critical",
        glass_paint("●", SUCCESS_COLOR, c),
        glass_paint(&r.summary.excellent.to_string(), SUCCESS_COLOR, c),
        glass_paint(&r.summary.healthy.to_string(), GLASS_BASE, c),
        glass_paint(&r.summary.warning.to_string(), ACCENT_COLOR, c),
        glass_paint(
            &r.summary.critical.to_string(),
            severity_code(Severity::Critical),
            c
        )
    );

    if v == Verbosity::Quiet {
        return out;
    }

    if r.findings.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{}",
            glass_paint(
                "✨ No modules over the entropy threshold.",
                SUCCESS_COLOR,
                c
            )
        );
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
        "{}",
        glass_paint(
            &format!(
                "{:<w_mod$}  {:>7}  {:>4}  {:<8}  {}",
                headers[0], headers[1], headers[2], headers[3], headers[4]
            ),
            HEADER_COLOR,
            c
        )
    );
    let rule_len = (w_mod + 2 + 7 + 2 + 4 + 2 + 8 + 2 + 30).min(style.width);
    let _ = writeln!(
        out,
        "{}",
        glass_paint(&"─".repeat(rule_len), GLASS_HIGHLIGHT, c)
    );

    for f in &r.findings {
        let path = ellipsize(&f.module.to_string_lossy(), w_mod);
        let path_colored = if f.severity >= Severity::Warning {
            glass_paint(&path, severity_code(f.severity), c)
        } else {
            glass_paint(&path, GLASS_BASE, c)
        };

        let entropy_colored = if f.entropy >= 0.8 {
            glass_paint(
                &format!("{:>7.2}", f.entropy),
                severity_code(Severity::Critical),
                c,
            )
        } else if f.entropy >= 0.65 {
            glass_paint(&format!("{:>7.2}", f.entropy), ACCENT_COLOR, c)
        } else {
            glass_paint(&format!("{:>7.2}", f.entropy), GLASS_BASE, c)
        };

        let sev = glass_paint(
            &format!("{:<8}", f.severity.label()),
            severity_code(f.severity),
            c,
        );

        let _ = writeln!(
            out,
            "{}  {}  {:>4}  {}  {}",
            path_colored,
            entropy_colored,
            glass_paint(&fmt_confidence_num(f.confidence), SUBTLE_COLOR, c),
            sev,
            glass_paint(&ellipsize(&f.next_action, 40), GLASS_HIGHLIGHT, c)
        );
    }

    let actionable: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::Warning)
        .collect();
    if !actionable.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{}",
            glass_border(&glass_paint("Findings", HEADER_COLOR, c), c)
        );
        for f in actionable {
            let badge = glass_paint(
                &format!("◈ {} ◈", f.severity.label()),
                severity_code(f.severity),
                c,
            );
            let _ = writeln!(
                out,
                "  {} {}",
                badge,
                glass_paint(&f.module.display().to_string(), GLASS_HIGHLIGHT, c)
            );
            let _ = writeln!(
                out,
                "      {} : {}",
                glass_paint("why", ACCENT_COLOR, c),
                glass_paint(&f.why, SUBTLE_COLOR, c)
            );
            let _ = writeln!(
                out,
                "      {} : {}",
                glass_paint("next", ACCENT_COLOR, c),
                glass_paint(&f.next_action, GLASS_BASE, c)
            );
            if v >= Verbosity::Verbose {
                for (k, val) in &f.evidence {
                    let _ = writeln!(
                        out,
                        "      {} : {}",
                        glass_paint(k, ACCENT_COLOR, c),
                        glass_paint(val, SUBTLE_COLOR, c)
                    );
                }
            }
        }
    }

    out
}
