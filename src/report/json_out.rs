//! JSON and JSONL renderers.

use crate::json::Value;

use super::model::{Finding, Report, Summary};

// ---------------------------------------------------------------------------
// JSON / JSONL
// ---------------------------------------------------------------------------

pub(crate) fn sv(s: impl Into<String>) -> Value {
    Value::String(s.into())
}

fn summary_value(s: &Summary) -> Value {
    let mut o = Value::object();
    o.insert("total", Value::Number(s.total as f64));
    o.insert("excellent", Value::Number(s.excellent as f64));
    o.insert("healthy", Value::Number(s.healthy as f64));
    o.insert("warning", Value::Number(s.warning as f64));
    o.insert("critical", Value::Number(s.critical as f64));
    o.insert("score", Value::Number(s.score));
    o
}

fn finding_value(f: &Finding) -> Value {
    let mut o = Value::object();
    o.insert("id", sv(&f.id));
    o.insert("severity", sv(f.severity.label()));
    o.insert("module", sv(f.module.to_string_lossy()));
    o.insert("kind", sv(&f.kind));
    o.insert("entropy", Value::Number(f.entropy));
    o.insert(
        "confidence",
        f.confidence.map_or(Value::Null, Value::Number),
    );
    o.insert("message", sv(&f.message));
    o.insert("why", sv(&f.why));
    o.insert("next_action", sv(&f.next_action));
    let mut ev = Value::object();
    for (k, v) in &f.evidence {
        ev.insert(k.clone(), sv(v));
    }
    o.insert("evidence", ev);
    o
}

pub(crate) fn render_json(r: &Report) -> Value {
    let mut o = Value::object();
    o.insert("schema", sv(r.schema));
    o.insert("root", sv(r.root.to_string_lossy()));
    o.insert("summary", summary_value(&r.summary));
    o.insert(
        "findings",
        Value::Array(r.findings.iter().map(finding_value).collect()),
    );
    o
}

pub(crate) fn render_jsonl(r: &Report) -> String {
    r.findings
        .iter()
        .map(|f| finding_value(f).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
