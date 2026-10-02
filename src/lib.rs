//! Terror Bat — falsification and assurance framework.
//!
//! M1: Bat Spec parsing / parameter resolution / canonicalisation / identity.
//! M2: supervised process-tree execution.
//! M3+M4: disposable worktree execution, built-in primitives, durable
//! content-addressed evidence and operation log.
//!
//! Pipeline: PARSE → RESOLVE → CANONICALISE → IDENTIFY → RUN → EVIDENCE.

pub mod builtins;
pub mod canonical;
pub mod doctor;
pub mod error;
pub mod evidence;
pub mod identity;
pub mod numeric;
pub mod oracle;
pub mod params;
pub mod presentation;
pub mod receipt;
pub mod runner;
pub mod spec;
pub mod strict;
pub mod supervisor;
pub mod worktree;

use std::path::Path;

pub use error::{Error, Result};
pub use identity::Identities;
pub use params::ParamOverrides;
pub use spec::BatSpec;

/// Everything a human or test needs from a successful identification run.
#[derive(Debug, Clone)]
pub struct IdentifiedSpec {
    /// Human label (`id`); not part of Bat identity.
    pub human_id: String,
    /// Exact RFC 8785 canonical JSON of the semantic projection.
    pub canonical_json: String,
    pub identities: Identities,
}

/// Read, parse, resolve, canonicalise, and identify a Bat Spec file.
pub fn identify_spec_file(path: &Path, overrides: &ParamOverrides) -> Result<IdentifiedSpec> {
    let yaml = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    identify_spec_str(&yaml, path, overrides)
}

/// Parse, resolve, canonicalise, and identify Bat Spec YAML source.
pub fn identify_spec_str(
    yaml: &str,
    path: &Path,
    overrides: &ParamOverrides,
) -> Result<IdentifiedSpec> {
    let spec = spec::parse_spec(yaml, path, overrides)?;
    identify_spec(&spec)
}

/// Canonicalise and identify an already-parsed Bat Spec.
pub fn identify_spec(spec: &BatSpec) -> Result<IdentifiedSpec> {
    let projection = canonical::projection(spec);
    let bytes = canonical::canonical_json(&projection)?;
    let canonical_json = String::from_utf8(bytes).expect("canonical JSON is valid UTF-8");
    let identities = identity::identities(&projection)?;
    Ok(IdentifiedSpec {
        human_id: spec.id.clone(),
        canonical_json,
        identities,
    })
}

/// Validate a Bat Spec file without producing identities beyond validity.
/// Parameters resolve from declared defaults only.
pub fn check_spec_file(path: &Path) -> Result<String> {
    let yaml = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let spec = spec::parse_spec(&yaml, path, &ParamOverrides::default())?;
    Ok(spec.id)
}
