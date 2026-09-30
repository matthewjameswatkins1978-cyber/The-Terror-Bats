//! Semantic projection and RFC 8785 canonical JSON.
//!
//! The canonical JSON bytes are part of the M1 contract: they are inspectable
//! (`terrorbat spec canonical`) and are what gets hashed. No Unicode
//! normalisation is performed; parsed string content is canonicalised exactly
//! as represented by the resolved semantic structure.

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::spec::BatSpec;

/// Build the semantic projection of a resolved Bat Spec.
///
/// Includes: version, claim, requires, forbids, environment.relevant,
/// attack, oracle, evidence, timeout.
///
/// Excludes: id, meta, params declarations, source filename, comments,
/// YAML formatting.
///
/// Set-like collections (`requires`, `forbids`, `environment.relevant`,
/// `evidence.capture`) are sorted and deduplicated. Order is preserved where
/// it can be meaningful (`attack.setup`, `attack.run`, oracle child arrays).
/// No logical-equivalence reasoning is attempted on oracle expressions.
pub fn projection(spec: &BatSpec) -> Value {
    let mut m = Map::new();
    m.insert("version".into(), Value::String(spec.version.clone()));
    m.insert("claim".into(), claim_json(spec));
    m.insert("requires".into(), capability_set_json(&spec.requires));
    m.insert("forbids".into(), capability_set_json(&spec.forbids));
    if let Some(env) = &spec.environment {
        let relevant = sorted_deduped(&env.relevant);
        if !relevant.is_empty() {
            let mut env_map = Map::new();
            env_map.insert(
                "relevant".into(),
                Value::Array(relevant.into_iter().map(Value::String).collect()),
            );
            m.insert("environment".into(), Value::Object(env_map));
        }
    }
    m.insert("attack".into(), attack_json(spec));
    m.insert("oracle".into(), spec.oracle.0.clone());
    m.insert("evidence".into(), evidence_json(spec));
    if let Some(timeout) = timeout_json(spec) {
        m.insert("timeout".into(), timeout);
    }
    Value::Object(m)
}

fn claim_json(spec: &BatSpec) -> Value {
    let mut m = Map::new();
    m.insert("text".into(), Value::String(spec.claim.text.clone()));
    Value::Object(m)
}

fn attack_json(spec: &BatSpec) -> Value {
    let mut m = Map::new();
    m.insert("setup".into(), steps_json(&spec.attack.setup));
    m.insert("run".into(), steps_json(&spec.attack.run));
    Value::Object(m)
}

fn steps_json(steps: &[crate::spec::Step]) -> Value {
    Value::Array(
        steps
            .iter()
            .map(|step| {
                let mut m = Map::new();
                m.insert("adapter".into(), Value::String(step.adapter.clone()));
                m.insert("action".into(), Value::String(step.action.clone()));
                for (key, value) in &step.payload {
                    m.insert(key.clone(), value.clone());
                }
                Value::Object(m)
            })
            .collect(),
    )
}

fn evidence_json(spec: &BatSpec) -> Value {
    let mut m = Map::new();
    m.insert("capture".into(), set_json(&spec.evidence.capture));
    Value::Object(m)
}

fn timeout_json(spec: &BatSpec) -> Option<Value> {
    let timeouts = spec.timeout.as_ref()?;
    let mut m = Map::new();
    for (key, value) in [
        ("setup", timeouts.setup),
        ("run", timeouts.run),
        ("oracle", timeouts.oracle),
        ("total", timeouts.total),
    ] {
        if let Some(t) = value {
            m.insert(key.into(), Value::Number(t.0.into()));
        }
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

fn set_json(entries: &[String]) -> Value {
    Value::Array(
        sorted_deduped(entries)
            .into_iter()
            .map(Value::String)
            .collect(),
    )
}

fn capability_set_json(entries: &[crate::spec::Capability]) -> Value {
    let strings: Vec<String> = entries.iter().map(|c| c.0.clone()).collect();
    set_json(&strings)
}

fn sorted_deduped(entries: &[String]) -> Vec<String> {
    let mut v: Vec<String> = entries.to_vec();
    v.sort();
    v.dedup();
    v
}

/// Serialise a value to RFC 8785 canonical JSON bytes.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    use std::path::Path;
    serde_json_canonicalizer::to_vec(value).map_err(|e| {
        Error::spec(
            Path::new("<canonicalisation>"),
            format!("value cannot be represented as RFC 8785 canonical JSON: {e}"),
        )
    })
}
