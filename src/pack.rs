//! Bat Pack v1: strict manifests that name an ordered list of Bats.
//!
//! A Pack is a manifest, not a program. It declares `version`, a human
//! `id`, an optional human `description`, and a non-empty ordered `bats`
//! list. Each entry names a Bat Spec file (relative to the pack file) plus
//! optional per-entry parameter VALUES (scalars only — the pack language
//! has no `$param` templates and performs no second substitution).
//!
//! Pack identity (`pack:sha256:<hex>`) covers exactly `{version, bats}`
//! where `bats` is the ordered list of effective Bat identities ( Bat
//! content resolved with the entry's params). Human metadata (`id`,
//! `description`) and YAML formatting never affect identity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::{canonical, evidence, numeric};

/// The only Bat Pack schema version accepted by W2 MS1.
pub const PACK_VERSION: &str = "terrorbat-pack/v1";

/// Strict pack manifest shape. Unknown fields are rejected on deserialise.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackManifest {
    version: String,
    /// Human label. NOT part of pack identity, never execution input.
    id: String,
    /// Human text. NOT part of pack identity, never execution input.
    /// Parsed (so non-string values are rejected) but never read beyond
    /// that: its only role is schema strictness.
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
    bats: Vec<PackEntry>,
}

/// One pack entry: a Bat file plus optional parameter VALUES.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackEntry {
    path: String,
    #[serde(default)]
    params: BTreeMap<String, Value>,
}

/// One resolved pack entry: the effective Bat identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEntry {
    /// Entry path exactly as written in the pack manifest.
    pub path: String,
    /// Bat file resolved against the pack file's directory.
    pub resolved: PathBuf,
    /// Human label of the effective Bat (not part of pack identity).
    pub human_id: String,
    /// Effective Bat content identity (`bat:sha256:...`, params resolved).
    pub bat: String,
    /// Per-entry parameter VALUES rendered as `NAME=VALUE` override strings
    /// (sorted by name), ready for `ParamOverrides::parse` / `run_bat`.
    pub overrides: Vec<String>,
}

/// Everything a human or test needs from a successful pack identification.
#[derive(Debug, Clone)]
pub struct IdentifiedPack {
    /// Human label (`id`); not part of pack identity.
    pub human_id: String,
    /// `pack:sha256:<hex>` over the canonical `{version, bats}` projection.
    pub identity: String,
    /// Exact canonical JSON of the `{version, bats}` projection.
    pub canonical_json: String,
    /// Effective Bat identities in pack order.
    pub entries: Vec<ResolvedEntry>,
}

/// Plain-schema JSON view of an identified pack (for `--json` output).
/// This is schema, never presentation: no styling, no capability probing.
#[derive(Debug, Clone, Serialize)]
pub struct PackJson {
    pub id: String,
    pub identity: String,
    pub bats: Vec<PackEntryJson>,
}

/// Plain-schema JSON view of one resolved pack entry.
#[derive(Debug, Clone, Serialize)]
pub struct PackEntryJson {
    pub path: String,
    pub id: String,
    pub bat: String,
}

impl IdentifiedPack {
    pub fn to_json(&self) -> PackJson {
        PackJson {
            id: self.human_id.clone(),
            identity: self.identity.clone(),
            bats: self
                .entries
                .iter()
                .map(|e| PackEntryJson {
                    path: e.path.clone(),
                    id: e.human_id.clone(),
                    bat: e.bat.clone(),
                })
                .collect(),
        }
    }
}

/// Parse YAML source into an untyped pack document.
///
/// Mirrors `spec::parse_yaml_value`: strict string keys, duplicate-key and
/// multi-document rejection inherited from `serde-saphyr`, top-level mapping
/// required. Integer bounds are enforced everywhere (a pack has no bat-level
/// `params` declaration block that would need parameter-aware leniency);
/// floats survive this stage and are rejected with pack-aware errors in
/// `validate_entry_params`.
fn parse_pack_value(yaml: &str, path: &Path) -> Result<Value> {
    let value: crate::strict::StrictValue = serde_saphyr::from_str(yaml)
        .map_err(|e| Error::spec(path, yaml_message(&e.to_string())))?;
    let value = value.0;
    if !value.is_object() {
        return Err(Error::spec(
            path,
            "a Bat Pack must be a YAML mapping at the top level",
        ));
    }
    Ok(value)
}

/// Parse and validate a pack manifest (no Bat files are read).
fn parse_manifest(yaml: &str, path: &Path) -> Result<PackManifest> {
    let doc = parse_pack_value(yaml, path)?;
    let manifest: PackManifest = serde_json::from_value(doc)
        .map_err(|e| Error::spec(path, format!("schema violation: {e}")))?;
    validate_manifest(&manifest, path)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &PackManifest, path: &Path) -> Result<()> {
    if manifest.version != PACK_VERSION {
        return Err(Error::spec(
            path,
            format!(
                "unsupported Bat Pack version `{}` (expected `{}`)",
                manifest.version, PACK_VERSION
            ),
        ));
    }
    if manifest.id.trim().is_empty() {
        return Err(Error::spec(path, "`id` must be a non-empty human label"));
    }
    if manifest.bats.is_empty() {
        return Err(Error::spec(
            path,
            "`bats` must be a non-empty list of Bat entries",
        ));
    }
    for entry in &manifest.bats {
        if entry.path.trim().is_empty() {
            return Err(Error::spec(
                path,
                "`bats` entries need a non-empty `path` to a Bat Spec file",
            ));
        }
        validate_entry_params(&entry.params, &entry.path, path)?;
    }
    Ok(())
}

/// Validate per-entry params: string/bool/safe-int scalars only.
/// Returns canonical `NAME=VALUE` override strings in entry order
/// (`BTreeMap` iteration = sorted by name, hence deterministic).
fn validate_entry_params(
    params: &BTreeMap<String, Value>,
    entry_path: &str,
    pack_path: &Path,
) -> Result<Vec<String>> {
    let mut overrides = Vec::with_capacity(params.len());
    for (name, value) in params {
        if name.is_empty() {
            return Err(Error::spec(
                pack_path,
                format!("entry `{entry_path}` has a parameter with an empty name"),
            ));
        }
        let rendered = match value {
            Value::String(s) => format!("{name}={s}"),
            Value::Bool(b) => format!("{name}={b}"),
            Value::Number(n) => match n.as_i64() {
                Some(i) if numeric::is_safe_i64(i) => format!("{name}={i}"),
                Some(i) => {
                    return Err(Error::spec(
                        pack_path,
                        format!(
                            "entry `{entry_path}` parameter `{name}` value {i} exceeds the \
                             Bat Spec v1 JCS-safe range ({}..={}); encode larger exact \
                             integers as strings",
                            numeric::MIN_SAFE_INTEGER,
                            numeric::MAX_SAFE_INTEGER
                        ),
                    ));
                }
                None => {
                    return Err(Error::spec(
                        pack_path,
                        format!(
                            "entry `{entry_path}` parameter `{name}` must be a string, bool, \
                             or safe integer scalar; found a non-integer number"
                        ),
                    ));
                }
            },
            Value::Null => {
                return Err(Error::spec(
                    pack_path,
                    format!(
                        "entry `{entry_path}` parameter `{name}` must be a string, bool, or \
                         safe integer scalar; found null"
                    ),
                ));
            }
            Value::Array(_) => {
                return Err(Error::spec(
                    pack_path,
                    format!(
                        "entry `{entry_path}` parameter `{name}` must be a string, bool, or \
                         safe integer scalar; found a sequence"
                    ),
                ));
            }
            Value::Object(_) => {
                return Err(Error::spec(
                    pack_path,
                    format!(
                        "entry `{entry_path}` parameter `{name}` must be a string, bool, or \
                         safe integer scalar; found a mapping"
                    ),
                ));
            }
        };
        overrides.push(rendered);
    }
    Ok(overrides)
}

/// Build the pack identity projection: `{version, bats}` where `bats` is
/// the ordered list of effective Bat identities.
pub fn projection(bat_ids: &[String]) -> Value {
    let mut m = Map::new();
    m.insert("version".into(), Value::String(PACK_VERSION.to_string()));
    m.insert(
        "bats".into(),
        Value::Array(bat_ids.iter().map(|s| Value::String(s.clone())).collect()),
    );
    Value::Object(m)
}

/// Canonicalise the projection and compute `pack:sha256:<hex>`.
/// Returns `(canonical_json, identity)`.
pub fn pack_identity(bat_ids: &[String]) -> Result<(String, String)> {
    let bytes = canonical::canonical_json(&projection(bat_ids))?;
    let canonical_json = String::from_utf8(bytes).expect("canonical JSON is valid UTF-8");
    let hex = evidence::sha256_hex(canonical_json.as_bytes());
    Ok((canonical_json, format!("pack:sha256:{hex}")))
}

/// Read, validate, resolve, and identify a Bat Pack file.
///
/// Entry `path`s resolve against the pack file's directory. Each entry's
/// params render to `NAME=VALUE` strings and flow through the existing
/// `ParamOverrides::parse` + `identify_spec_file`, so the effective Bat
/// identity includes the params. No second substitution system exists.
pub fn identify_pack_file(path: &Path) -> Result<IdentifiedPack> {
    let yaml = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let manifest = parse_manifest(&yaml, path)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));

    let mut entries = Vec::with_capacity(manifest.bats.len());
    let mut bat_ids = Vec::with_capacity(manifest.bats.len());
    for entry in &manifest.bats {
        let overrides_raw = validate_entry_params(&entry.params, &entry.path, path)?;
        let overrides = crate::params::ParamOverrides::parse(&overrides_raw).map_err(|e| {
            Error::spec(
                path,
                format!("entry `{}` has invalid params: {e}", entry.path),
            )
        })?;
        let resolved = base.join(&entry.path);
        let identified = crate::identify_spec_file(&resolved, &overrides).map_err(|e| {
            Error::spec(
                path,
                format!("entry `{}` is not a valid effective Bat: {e}", entry.path),
            )
        })?;
        bat_ids.push(identified.identities.bat.clone());
        entries.push(ResolvedEntry {
            path: entry.path.clone(),
            resolved,
            human_id: identified.human_id,
            bat: identified.identities.bat,
            overrides: overrides_raw,
        });
    }

    let (canonical_json, identity) = pack_identity(&bat_ids)?;
    Ok(IdentifiedPack {
        human_id: manifest.id,
        identity,
        canonical_json,
        entries,
    })
}

/// Validate a Bat Pack file (plus all effective Bats) without returning
/// identities beyond validity.
pub fn check_pack_file(path: &Path) -> Result<String> {
    Ok(identify_pack_file(path)?.human_id)
}

/// Condense serde-saphyr's multi-line error rendering into its first line,
/// keeping messages human-readable (mirrors `spec::yaml_message`).
fn yaml_message(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or(raw);
    first
        .strip_prefix("error: ")
        .unwrap_or(first)
        .trim()
        .to_string()
}
