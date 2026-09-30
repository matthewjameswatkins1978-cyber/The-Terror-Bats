//! Parameter model: declarations, overrides, resolution, and `$param` substitution.
//!
//! Parameters are explicit typed values, not a template language:
//! no expressions, no environment-variable interpolation, no `${foo}`
//! substitution inside strings. A reference is an explicit data node
//! `{ "$param": "<name>" }` that must be the only key of its mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::error::{Error, Result};

/// Declared parameter types (M1: no floats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Bool,
    Integer,
}

impl ParamType {
    fn name(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Bool => "bool",
            ParamType::Integer => "integer",
        }
    }
}

/// A resolved parameter value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    Str(String),
    Bool(bool),
    Int(i64),
}

impl ParamValue {
    pub fn to_json(&self) -> Value {
        match self {
            ParamValue::Str(s) => Value::String(s.clone()),
            ParamValue::Bool(b) => Value::Bool(*b),
            ParamValue::Int(i) => Value::Number((*i).into()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamDecl {
    #[serde(rename = "type")]
    pub ty: ParamType,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub required: bool,
}

/// Parsed `--param name=value` overrides.
#[derive(Debug, Default, Clone)]
pub struct ParamOverrides {
    entries: Vec<(String, String)>,
}

impl ParamOverrides {
    /// Parse `name=value` pairs. Rejects malformed pairs and duplicate names.
    pub fn parse(raw: &[String]) -> Result<Self> {
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        for item in raw {
            let (name, value) = item.split_once('=').ok_or_else(|| Error::Spec {
                path: Path::new("<cli>").to_path_buf(),
                message: format!("malformed override `{item}`: expected `name=value`"),
            })?;
            if name.is_empty() {
                return Err(Error::Spec {
                    path: Path::new("<cli>").to_path_buf(),
                    message: format!("malformed override `{item}`: parameter name is empty"),
                });
            }
            if !seen.insert(name.to_string()) {
                return Err(Error::Spec {
                    path: Path::new("<cli>").to_path_buf(),
                    message: format!("duplicate override for parameter `{name}`"),
                });
            }
            entries.push((name.to_string(), value.to_string()));
        }
        Ok(ParamOverrides { entries })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }
}

struct WalkCtx<'a> {
    path: &'a Path,
    declared: &'a BTreeMap<String, ParamDecl>,
    values: &'a BTreeMap<String, ParamValue>,
}

impl WalkCtx<'_> {
    fn err(&self, message: impl Into<String>) -> Error {
        Error::spec(self.path, message)
    }
}

/// Resolve parameters for a parsed (untyped) spec document.
///
/// Removes the `params` declaration block from `doc`, computes the resolved
/// parameter values (CLI override beats declared default; a missing value for
/// a required parameter is an error), and replaces every `$param` reference
/// node in the remaining document with its resolved value.
pub fn resolve_params(
    doc: &mut Map<String, Value>,
    overrides: &ParamOverrides,
    path: &Path,
) -> Result<()> {
    let decls: BTreeMap<String, ParamDecl> = match doc.remove("params") {
        None => BTreeMap::new(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| Error::spec(path, format!("invalid `params` block: {e}")))?,
    };

    for name in overrides.names() {
        if !decls.contains_key(name) {
            return Err(Error::spec(
                path,
                format!("override for undeclared parameter `{name}`"),
            ));
        }
    }

    let mut values: BTreeMap<String, ParamValue> = BTreeMap::new();
    for (name, decl) in &decls {
        let value = if let Some(raw) = overrides.get(name) {
            Some(coerce_override(name, decl.ty, raw, path)?)
        } else if let Some(default) = &decl.default {
            Some(coerce_default(name, decl.ty, default, path)?)
        } else {
            None
        };
        match value {
            Some(v) => {
                values.insert(name.clone(), v);
            }
            None if decl.required => {
                return Err(Error::spec(
                    path,
                    format!("required parameter `{name}` has no value (no default, no override)"),
                ));
            }
            None => {}
        }
    }

    let ctx = WalkCtx {
        path,
        declared: &decls,
        values: &values,
    };
    for (_key, child) in doc.iter_mut() {
        walk(&ctx, child)?;
    }
    Ok(())
}

fn coerce_override(name: &str, ty: ParamType, raw: &str, path: &Path) -> Result<ParamValue> {
    match ty {
        ParamType::String => Ok(ParamValue::Str(raw.to_string())),
        ParamType::Bool => match raw {
            "true" => Ok(ParamValue::Bool(true)),
            "false" => Ok(ParamValue::Bool(false)),
            other => Err(Error::spec(
                path,
                format!("parameter `{name}` has type bool but override value `{other}` is not `true` or `false`"),
            )),
        },
        ParamType::Integer => raw.parse::<i64>().map(ParamValue::Int).map_err(|_| {
            Error::spec(
                path,
                format!("parameter `{name}` has type integer but override value `{raw}` is not an integer"),
            )
        }),
    }
}

fn coerce_default(name: &str, ty: ParamType, default: &Value, path: &Path) -> Result<ParamValue> {
    let mismatch = |actual: &str| {
        Error::spec(
            path,
            format!(
                "parameter `{name}` has type {} but its default is {actual}",
                ty.name()
            ),
        )
    };
    match (ty, default) {
        (ParamType::String, Value::String(s)) => Ok(ParamValue::Str(s.clone())),
        (ParamType::Bool, Value::Bool(b)) => Ok(ParamValue::Bool(*b)),
        (ParamType::Integer, Value::Number(n)) => n
            .as_i64()
            .map(ParamValue::Int)
            .ok_or_else(|| mismatch("a number outside the integer range or a float")),
        (_, Value::String(_)) => Err(mismatch("a string")),
        (_, Value::Bool(_)) => Err(mismatch("a bool")),
        (_, Value::Number(_)) => Err(mismatch("a number")),
        (_, Value::Null) => Err(mismatch("null")),
        (_, Value::Array(_)) => Err(mismatch("a sequence")),
        (_, Value::Object(_)) => Err(mismatch("a mapping")),
    }
}

fn walk(ctx: &WalkCtx<'_>, node: &mut Value) -> Result<()> {
    match node {
        Value::Array(items) => {
            for item in items {
                walk(ctx, item)?;
            }
        }
        Value::Object(map) => {
            if map.contains_key("$param") {
                if map.len() != 1 {
                    return Err(ctx.err(
                        "a `$param` node must contain exactly one key (`$param`); found additional keys",
                    ));
                }
                let name = map["$param"].as_str().ok_or_else(|| {
                    ctx.err("`$param` value must be the name of a declared parameter (a string)")
                })?;
                let value = match ctx.values.get(name) {
                    Some(v) => v.clone(),
                    None if ctx.declared.contains_key(name) => {
                        return Err(ctx.err(format!(
                            "parameter reference `$param: {name}` cannot be resolved: \
                             required parameter `{name}` has no value (no default, no override)"
                        )));
                    }
                    None => {
                        return Err(ctx.err(format!(
                            "parameter reference `$param: {name}` refers to an undeclared parameter"
                        )));
                    }
                };
                *node = value.to_json();
            } else {
                for (_key, child) in map.iter_mut() {
                    walk(ctx, child)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
