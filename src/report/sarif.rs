//! SARIF 2.1.0 renderer.

use crate::json::Value;
use crate::RefactorKind;

use super::json_out::sv;
use super::model::{Finding, Report, Severity};

// ---------------------------------------------------------------------------
// SARIF 2.1.0
// ---------------------------------------------------------------------------

fn rule_id(kind: &str) -> String {
    kind.to_ascii_lowercase().replace(' ', "-")
}

fn sarif_rules() -> Vec<Value> {
    let kinds = [
        RefactorKind::SplitModule,
        RefactorKind::ExtractFunction,
        RefactorKind::RemoveDuplication,
        RefactorKind::ReduceSurface,
        RefactorKind::ReorderDependencies,
    ];
    kinds
        .iter()
        .map(|k| {
            let mut r = Value::object();
            r.insert("id", sv(rule_id(k.description())));
            let mut sd = Value::object();
            sd.insert("text", sv(k.description()));
            r.insert("shortDescription", sd);
            r
        })
        .collect()
}

fn sarif_result(f: &Finding) -> Value {
    let mut r = Value::object();
    r.insert("ruleId", sv(rule_id(&f.kind)));
    r.insert("level", sv(f.severity.sarif_level()));
    let mut msg = Value::object();
    msg.insert("text", sv(format!("{}. {}", f.message, f.why)));
    r.insert("message", msg);

    let mut region = Value::object();
    region.insert("startLine", Value::Number(1.0));
    let mut artifact = Value::object();
    artifact.insert("uri", sv(f.module.to_string_lossy()));
    let mut phys = Value::object();
    phys.insert("artifactLocation", artifact);
    phys.insert("region", region);
    let mut loc = Value::object();
    loc.insert("physicalLocation", phys);
    r.insert("locations", Value::Array(vec![loc]));

    let mut props = Value::object();
    props.insert("entropy", Value::Number(f.entropy));
    props.insert(
        "confidence",
        f.confidence.map_or(Value::Null, Value::Number),
    );
    props.insert("next_action", sv(&f.next_action));
    r.insert("properties", props);
    r
}

pub(crate) fn render_sarif(r: &Report) -> Value {
    let results: Vec<Value> = r
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::Warning)
        .map(sarif_result)
        .collect();

    let mut driver = Value::object();
    driver.insert("name", sv("fract"));
    driver.insert("version", sv(env!("CARGO_PKG_VERSION")));
    driver.insert("rules", Value::Array(sarif_rules()));
    let mut tool = Value::object();
    tool.insert("driver", driver);
    let mut run = Value::object();
    run.insert("tool", tool);
    run.insert("results", Value::Array(results));

    let mut top = Value::object();
    top.insert("version", sv("2.1.0"));
    top.insert(
        "$schema",
        sv("https://json.schemastore.org/sarif-2.1.0.json"),
    );
    top.insert("runs", Value::Array(vec![run]));
    top
}
