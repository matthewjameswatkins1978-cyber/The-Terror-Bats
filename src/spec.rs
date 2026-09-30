//! Typed Bat Spec model and the YAML → typed parse pipeline.
//!
//! Core structures reject unknown fields. Adapter step payloads, the oracle
//! expression, and `meta` remain opaque structured data: their contracts are
//! deliberately deferred (oracle registry → M5, adapter protocol → M8).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::params::{self, ParamOverrides};

/// The only Bat Spec schema version accepted by M1.
pub const SPEC_VERSION: &str = "terrorbat/v1";

/// A capability declaration: either a plain string (`fs.read`, `network`)
/// or a single YAML mapping entry (`fs.write: fixture/**`), which M0 writes
/// unquoted. Both forms normalize to one canonical string (`name: scope`),
/// so formatting choice never affects Bat identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Capability(pub String);

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(s) => non_empty_capability(&s).map_err(serde::de::Error::custom),
            Value::Object(map) => {
                if map.len() != 1 {
                    return Err(serde::de::Error::custom(
                        "a scoped capability entry must be a single `name: scope` mapping",
                    ));
                }
                let (name, scope) = map.into_iter().next().expect("len checked");
                let scope = scope.as_str().ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        "capability `{name}` has a non-string scope; scopes must be strings"
                    ))
                })?;
                non_empty_capability(&format!("{name}: {scope}")).map_err(serde::de::Error::custom)
            }
            other => Err(serde::de::Error::custom(format!(
                "capability entries must be strings or `name: scope` mappings, found {}",
                kind_name(&other)
            ))),
        }
    }
}

impl Serialize for Capability {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

fn non_empty_capability(s: &str) -> std::result::Result<Capability, String> {
    if s.trim().is_empty() {
        return Err("capability entries must be non-empty strings".to_string());
    }
    Ok(Capability(s.to_string()))
}

fn kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a sequence",
        Value::Object(_) => "a mapping",
    }
}

/// A parsed, validated, parameter-resolved Bat Spec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatSpec {
    pub version: String,
    /// Human label. NOT part of canonical Bat identity.
    pub id: String,
    pub claim: Claim,
    #[serde(default)]
    pub requires: Vec<Capability>,
    #[serde(default)]
    pub forbids: Vec<Capability>,
    #[serde(default)]
    pub environment: Option<Environment>,
    pub attack: Attack,
    pub oracle: Oracle,
    pub evidence: Evidence,
    #[serde(default)]
    pub timeout: Option<Timeouts>,
    /// Free-form metadata. NOT part of canonical Bat identity.
    #[serde(default)]
    pub meta: Option<Meta>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// Symbolic names of environment facts that may matter to
    /// reproducibility. Declarations are part of Bat identity; actual
    /// machine values are never inspected or hashed in M1.
    pub relevant: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attack {
    #[serde(default)]
    pub setup: Vec<Step>,
    #[serde(default)]
    pub run: Vec<Step>,
}

/// One adapter step. `adapter` and `action` are typed; all other keys are an
/// opaque adapter payload (language/tool contracts are deferred to M8).
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    pub adapter: String,
    pub action: String,
    #[serde(flatten)]
    pub payload: BTreeMap<String, Value>,
}

/// Opaque deterministic-oracle expression. Must be a mapping. The exact
/// condition-type registry is an M5 decision; M1 only requires structured
/// data so that `$param` resolution and canonicalisation work uniformly.
#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Oracle(pub Value);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub capture: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Timeouts {
    #[serde(default)]
    pub setup: Option<TimeoutSeconds>,
    #[serde(default)]
    pub run: Option<TimeoutSeconds>,
    #[serde(default)]
    pub oracle: Option<TimeoutSeconds>,
    #[serde(default)]
    pub total: Option<TimeoutSeconds>,
}

/// A timeout in the one strict Bat Spec v1 representation: `<unsigned integer>s`.
/// Ambiguous aliases (`10m`, `0.5h`, `600000ms`) are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TimeoutSeconds(pub u64);

impl<'de> Deserialize<'de> for TimeoutSeconds {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        parse_timeout_seconds(&raw)
            .map(TimeoutSeconds)
            .map_err(serde::de::Error::custom)
    }
}

/// Parse the strict `<unsigned integer>s` timeout form into integer seconds.
pub fn parse_timeout_seconds(raw: &str) -> std::result::Result<u64, String> {
    let bad = || {
        format!(
            "invalid timeout `{raw}`: Bat Spec v1 accepts only the strict \
             `<unsigned integer>s` form (e.g. `600s`)"
        )
    };
    let digits = raw.strip_suffix('s').ok_or_else(bad)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    digits.parse::<u64>().map_err(|_| bad())
}

/// Free-form metadata block (labels only; excluded from Bat identity).
#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Meta(pub Value);

/// Parse YAML source into an untyped document, rejecting malformed input.
///
/// `serde-saphyr` rejects duplicate mapping keys and multiple YAML documents
/// by default; anchors/aliases resolve to the same typed representation as
/// their expanded equivalents.
pub fn parse_yaml_value(yaml: &str, path: &Path) -> Result<Value> {
    let value: Value = serde_saphyr::from_str(yaml)
        .map_err(|e| Error::spec(path, yaml_message(&e.to_string())))?;
    if !value.is_object() {
        return Err(Error::spec(
            path,
            "a Bat Spec must be a YAML mapping at the top level",
        ));
    }
    Ok(value)
}

/// Parse and validate a complete Bat Spec from YAML source:
/// parse → resolve parameters (`$param` nodes) → typed validation.
pub fn parse_spec(yaml: &str, path: &Path, overrides: &ParamOverrides) -> Result<BatSpec> {
    let mut doc = parse_yaml_value(yaml, path)?;
    let map = doc.as_object_mut().expect("checked in parse_yaml_value");
    params::resolve_params(map, overrides, path)?;

    let spec: BatSpec = serde_json::from_value(doc)
        .map_err(|e| Error::spec(path, format!("schema violation: {e}")))?;
    validate(&spec, path)?;
    Ok(spec)
}

fn validate(spec: &BatSpec, path: &Path) -> Result<()> {
    if spec.version != SPEC_VERSION {
        return Err(Error::spec(
            path,
            format!(
                "unsupported Bat Spec version `{}` (expected `{}`)",
                spec.version, SPEC_VERSION
            ),
        ));
    }
    if spec.id.trim().is_empty() {
        return Err(Error::spec(path, "`id` must be a non-empty human label"));
    }
    if spec.claim.text.trim().is_empty() {
        return Err(Error::spec(path, "`claim.text` must be a non-empty string"));
    }
    for (section, entries) in [("requires", &spec.requires), ("forbids", &spec.forbids)] {
        for entry in entries {
            if entry.0.trim().is_empty() {
                return Err(Error::spec(
                    path,
                    format!("`{section}` entries must be non-empty strings"),
                ));
            }
        }
    }
    if let Some(env) = &spec.environment {
        for entry in &env.relevant {
            if entry.trim().is_empty() {
                return Err(Error::spec(
                    path,
                    "`environment.relevant` entries must be non-empty symbolic names",
                ));
            }
        }
    }
    for entry in &spec.evidence.capture {
        if entry.trim().is_empty() {
            return Err(Error::spec(
                path,
                "`evidence.capture` entries must be non-empty strings",
            ));
        }
    }
    if !spec.oracle.0.is_object() {
        return Err(Error::spec(path, "`oracle` must be a structured mapping"));
    }
    for (section, steps) in [
        ("attack.setup", &spec.attack.setup),
        ("attack.run", &spec.attack.run),
    ] {
        for step in steps {
            if step.adapter.trim().is_empty() || step.action.trim().is_empty() {
                return Err(Error::spec(
                    path,
                    format!("`{section}` steps need non-empty `adapter` and `action` fields"),
                ));
            }
        }
    }
    if let Some(meta) = &spec.meta
        && !meta.0.is_object()
    {
        return Err(Error::spec(path, "`meta` must be a structured mapping"));
    }
    Ok(())
}

/// Condense serde-saphyr's multi-line error rendering into its first line
/// plus location, keeping messages human-readable.
fn yaml_message(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or(raw);
    first
        .strip_prefix("error: ")
        .unwrap_or(first)
        .trim()
        .to_string()
}
