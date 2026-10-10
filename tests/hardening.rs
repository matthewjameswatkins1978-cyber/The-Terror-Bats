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

use terrorbats::ParamOverrides;

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

fn identify(yaml: &str) -> Result<terrorbats::IdentifiedSpec, String> {
    terrorbats::identify_spec_str(yaml, Path::new("<test>.yaml"), &ParamOverrides::default())
        .map_err(|e| e.to_string())
}

fn identify_with(yaml: &str, overrides: &[&str]) -> Result<terrorbats::IdentifiedSpec, String> {
    let owned: Vec<String> = overrides.iter().map(|s| s.to_string()).collect();
    let parsed = ParamOverrides::parse(&owned).map_err(|e| e.to_string())?;
    terrorbats::identify_spec_str(yaml, Path::new("<test>.yaml"), &parsed)
        .map_err(|e| e.to_string())
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
    let spec = terrorbats::spec::parse_spec(&yaml, path, &ParamOverrides::default())
        .unwrap_or_else(|e| panic!("quoted keys: {e}"));
    let meta = spec.meta.expect("meta present");
    assert_eq!(meta.0["1"], serde_json::Value::String("x".to_string()));
    assert_eq!(meta.0["true"], serde_json::Value::String("y".to_string()));
}

// ---------------------------------------------------------------------------
// Global JCS-safe integer boundary (M1.2).
//
// Every integer that reaches the parsed semantic document — opaque step
// payloads, oracle structures, meta, array elements, timeouts, parameter
// values — must fit ±(2^53−1). Otherwise two distinct integers could
// canonicalise to the same JSON number and share a content hash. Larger
// exact values must be quoted strings. Finite floats stay accepted per JCS.
// ---------------------------------------------------------------------------

/// Minimal spec with one comment placeholder per numeric entry point. A case
/// fills exactly one slot; the rest stay comments (valid empty sections).
const NUM_BASE: &str = "version: terrorbat/v1
id: num-probe
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      #STEP#
oracle:
  #ORACLE#
evidence: { capture: [stdout] }
meta:
  #META#
timeout:
  #TIMEOUT#
";

fn num_spec(step: &str, oracle: &str, meta: &str, timeout: &str) -> String {
    NUM_BASE
        .replace("#STEP#", step)
        .replace("#ORACLE#", oracle)
        .replace("#META#", meta)
        .replace("#TIMEOUT#", timeout)
}

fn num_rejected(yaml: &str, location: &str) -> String {
    match identify(yaml) {
        Ok(_) => panic!("{location}: unsafe integer was accepted but must be rejected"),
        Err(e) => {
            assert!(e.contains("JCS-safe range"), "{location}: {e}");
            e
        }
    }
}

#[test]
fn opaque_integer_above_jcs_safe_range_is_rejected() {
    // (location, filled spec)
    let cases = [
        (
            "step payload",
            num_spec("count: 9007199254740992", "all: []", "note: x", "run: 60s"),
        ),
        (
            "step payload negative",
            num_spec("count: -9007199254740992", "all: []", "note: x", "run: 60s"),
        ),
        (
            "nested step payload",
            num_spec(
                "nested: { deep: { count: 9007199254740992 } }",
                "all: []",
                "note: x",
                "run: 60s",
            ),
        ),
        (
            "array element",
            num_spec(
                "items: [1, 9007199254740992]",
                "all: []",
                "note: x",
                "run: 60s",
            ),
        ),
        (
            "oracle",
            num_spec(
                "note: x",
                "equals: { value: 9007199254740992 }",
                "note: x",
                "run: 60s",
            ),
        ),
        (
            "nested oracle",
            num_spec(
                "note: x",
                "any: [{ value: -9007199254740992 }]",
                "note: x",
                "run: 60s",
            ),
        ),
        (
            "meta",
            num_spec("note: x", "all: []", "job_id: 9007199254740992", "run: 60s"),
        ),
    ];
    for (location, yaml) in cases {
        num_rejected(&yaml, location);
    }
}

#[test]
fn integer_machine_extremes_rejected_in_opaque_data() {
    for extreme in [
        "9223372036854775807",
        "-9223372036854775808",
        "18446744073709551615",
    ] {
        let yaml = num_spec(
            &format!("count: {extreme}"),
            "all: []",
            "note: x",
            "run: 60s",
        );
        num_rejected(&yaml, &format!("step payload {extreme}"));
    }
}

#[test]
fn timeout_above_jcs_safe_range_is_rejected() {
    let yaml = num_spec("note: x", "all: []", "note: x", "run: 9007199254740992s");
    let err = num_rejected(&yaml, "timeout");
    assert!(err.contains("invalid timeout"), "{err}");
}

#[test]
fn safe_boundary_integers_accepted_everywhere() {
    for bound in ["9007199254740991", "-9007199254740991", "0", "42"] {
        let yaml = num_spec(
            &format!("count: {bound}"),
            &format!("equals: {{ value: {bound} }}"),
            &format!("job_id: {bound}"),
            "run: 9007199254740991s",
        );
        identify(&yaml).unwrap_or_else(|e| panic!("{bound}: {e}"));
    }
}

#[test]
fn large_exact_integer_string_is_preserved() {
    // Quoted large integers are strings, not integers: accepted, and they
    // stay strings through canonicalisation (proving no silent conversion).
    let yaml = num_spec(
        "count: \"9007199254740992\"",
        "all: []",
        "job_id: \"18446744073709551615\"",
        "run: 60s",
    );
    let identified = identify(&yaml).expect("quoted large integers are strings");
    assert!(
        identified
            .canonical_json
            .contains("\"count\":\"9007199254740992\""),
        "{}",
        identified.canonical_json
    );
}
