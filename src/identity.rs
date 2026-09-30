//! Content identities: `<kind>:sha256:<lowercase hex>` over RFC 8785
//! canonical JSON of the resolved semantic component.
//!
//! M1 produces bat/claim/attack/oracle identities only. Run identity,
//! evidence identity, receipt identity, and cache lookup belong to later
//! milestones and are deliberately absent.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::canonical;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identities {
    pub bat: String,
    pub claim: String,
    pub attack: String,
    pub oracle: String,
}

/// Compute the content identity `<prefix>:sha256:<lowercase hex>` of a value.
pub fn content_id(prefix: &str, value: &Value) -> Result<String> {
    let bytes = canonical::canonical_json(value)?;
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(format!("{prefix}:sha256:{hex}"))
}

/// Compute all M1 identities from the semantic projection.
pub fn identities(projection: &Value) -> Result<Identities> {
    Ok(Identities {
        bat: content_id("bat", projection)?,
        claim: content_id(
            "claim",
            projection
                .get("claim")
                .expect("projection always has claim"),
        )?,
        attack: content_id(
            "attack",
            projection
                .get("attack")
                .expect("projection always has attack"),
        )?,
        oracle: content_id(
            "oracle",
            projection
                .get("oracle")
                .expect("projection always has oracle"),
        )?,
    })
}
