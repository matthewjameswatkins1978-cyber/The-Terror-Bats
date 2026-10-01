//! M1.1 identity-hardening regression tests.
//!
//! Two edge cases that could silently corrupt content identity:
//! 1. integers outside the JCS-safe range (two distinct integers could
//!    canonicalise to the same JSON number and share a hash);
//! 2. non-string YAML mapping keys (silently stringified by the JSON layer,
//!    making `1: value` and `"1": value` identical).
//!
//! These tests exist so future agents understand *why* the restrictions exist.

use std::path::Path;

use terrorbat::ParamOverrides;

const SAFE_MAX: i64 = 9_007_199_254_740_991;
const SAFE_MIN: i64 = -9_007_199_254_740_991;

fn int_spec(default: &str) -> String {
    format!(
        r#"
version: terrorbat/v1
id: int-probe
claim: {{ text: claim }}
attack:
  run:
    - adapter: demo
      action: use
      count: {{ $param: num }}
oracle: {{ all: [] }}
evidence: {{ capture: [stdout] }}
params:
  num:
    type: integer
    default: {default}
"#
    )
}

fn identify(yaml: &str) -> Result<terrorbat::IdentifiedSpec, String> {
    terrorbat::identify_spec_str(yaml, Path::new("<test>.yaml"), &ParamOverrides::default())
        .map_err(|e| e.to_string())
}

fn identify_with(yaml: &str, overrides: &[&str]) -> Result<terrorbat::IdentifiedSpec, String> {
    let owned: Vec<String> = overrides.iter().map(|s| s.to_string()).collect();
    let parsed = ParamOverrides::parse(&owned).map_err(|e| e.to_string())?;
    terrorbat::identify_spec_str(yaml, Path::new("<test>.yaml"), &parsed).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// JCS-safe integers
// ---------------------------------------------------------------------------

#[test]
fn integer_at_jcs_safe_boundary_is_accepted() {
    for bound in [SAFE_MAX, SAFE_MIN] {
        // as a declared default
        let spec = int_spec(&bound.to_string());
        let identified = identify(&spec).unwrap_or_else(|e| panic!("{bound} as default: {e}"));
        assert!(identified.identities.bat.starts_with("bat:sha256:"));

        // as a CLI override (no default declared)
        let no_default = int_spec("1").replace("    default: 1\n", "");
        let identified = identify_with(&no_default, &[&format!("num={bound}")])
            .unwrap_or_else(|e| panic!("{bound} as override: {e}"));
        assert!(identified.identities.bat.starts_with("bat:sha256:"));
    }
}

#[test]
fn integer_above_jcs_safe_range_is_rejected() {
    for bad in [SAFE_MAX + 1, SAFE_MIN - 1] {
        // as a declared default
        let err = identify(&int_spec(&bad.to_string())).expect_err("must be rejected");
        assert!(
            err.contains(&format!(
                "parameter `num` value {bad} exceeds the Bat Spec v1 safe integer range"
            )),
            "{err}"
        );
        // as a CLI override
        let no_default = int_spec("1").replace("    default: 1\n", "");
        let err =
            identify_with(&no_default, &[&format!("num={bad}")]).expect_err("must be rejected");
        assert!(
            err.contains(&format!(
                "parameter `num` value {bad} exceeds the Bat Spec v1 safe integer range"
            )),
            "{err}"
        );
    }
}

#[test]
fn integer_i64_extremes_are_rejected() {
    for extreme in [i64::MAX, i64::MIN] {
        let err = identify(&int_spec(&extreme.to_string())).expect_err("must be rejected");
        assert!(
            err.contains("exceeds the Bat Spec v1 safe integer range"),
            "{err}"
        );
        let no_default = int_spec("1").replace("    default: 1\n", "");
        let err =
            identify_with(&no_default, &[&format!("num={extreme}")]).expect_err("must be rejected");
        assert!(
            err.contains("exceeds the Bat Spec v1 safe integer range"),
            "{err}"
        );
    }
}

#[test]
fn jcs_adjacent_integers_cannot_both_enter_identity() {
    // Regression: before M1.1, 9007199254740991 and 9007199254740992 were both
    // accepted as integers, yet RFC 8785 canonicalises both to the same
    // IEEE-754 double — one shared identity for two distinct experiments.
    // Now the boundary value is accepted and its neighbour is rejected, so
    // the pair can never both enter canonical identity.
    let ok = identify(&int_spec(&SAFE_MAX.to_string())).expect("boundary accepted");
    let err = identify(&int_spec(&(SAFE_MAX + 1).to_string())).expect_err("neighbour rejected");
    assert!(ok.identities.bat.starts_with("bat:sha256:"));
    assert!(err.contains("safe integer range"), "{err}");
}

// ---------------------------------------------------------------------------
// Non-string YAML mapping keys
// ---------------------------------------------------------------------------

const MINIMAL: &str = r#"
version: terrorbat/v1
id: key-probe
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: noop
oracle: { all: [] }
evidence: { capture: [stdout] }
"#;

#[test]
fn non_string_yaml_mapping_key_is_rejected() {
    // Each case is a complete spec whose only defect is one non-string key.
    // (location, spec yaml, expected error detail)
    let step_payload = MINIMAL.replace("      action: noop", "      action: noop\n      1: x");
    let oracle_payload = MINIMAL.replace("oracle: { all: [] }", "oracle: { 1: [] }");
    let cases = [
        (
            "top level",
            format!("{MINIMAL}1: value\n"),
            "integer key `1`",
        ),
        ("step payload", step_payload, "integer key `1`"),
        ("oracle payload", oracle_payload, "integer key `1`"),
        (
            "meta",
            format!("{MINIMAL}meta: {{ true: x }}\n"),
            "boolean key `true`",
        ),
        (
            "nested mapping",
            format!("{MINIMAL}meta: {{ outer: {{ null: x }} }}\n"),
            "null key",
        ),
        (
            "float key",
            format!("{MINIMAL}meta: {{ 1.5: x }}\n"),
            "float key `1.5`",
        ),
        (
            "sequence key",
            format!("{MINIMAL}meta: {{ [a, b]: x }}\n"),
            "sequence key",
        ),
        (
            "mapping key",
            format!("{MINIMAL}meta: {{ {{a: b}}: x }}\n"),
            "mapping key",
        ),
    ];
    for (location, yaml, detail) in cases {
        let err = match identify(&yaml) {
            Ok(_) => panic!("{location}: spec was accepted but must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.contains("YAML mapping keys must be strings"),
            "{location}: {err}"
        );
        assert!(err.contains(detail), "{location}: {err}");
    }
}

#[test]
fn quoted_numeric_string_key_is_accepted() {
    // A quoted numeric key is a genuine string and must keep working —
    // including inside opaque payloads (meta). Meta is excluded from
    // canonical identity by design, so assert on the parsed spec itself.
    // (Unquoted `y` as a *value* would resolve to boolean true under YAML 1.1
    // scalar rules — pre-existing scalar behaviour, unrelated to key strictness —
    // so the values are quoted as well.)
    let yaml = [MINIMAL, "meta: { \"1\": \"x\", \"true\": \"y\" }\n"].concat();
    let path = Path::new("<test>.yaml");
    let spec = terrorbat::spec::parse_spec(&yaml, path, &ParamOverrides::default())
        .unwrap_or_else(|e| panic!("quoted keys: {e}"));
    let meta = spec.meta.expect("meta present");
    assert_eq!(meta.0["1"], serde_json::Value::String("x".to_string()));
    assert_eq!(meta.0["true"], serde_json::Value::String("y".to_string()));
}
