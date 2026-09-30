//! Identity contract tests: irrelevant YAML differences must not change
//! identity; semantic differences must.

use std::path::{Path, PathBuf};

use terrorbat::{ParamOverrides, identify_spec_file};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn ids(name: &str) -> terrorbat::Identities {
    identify_spec_file(&fixture(name), &ParamOverrides::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .identities
}

fn ids_with(name: &str, overrides: &[&str]) -> terrorbat::Identities {
    let owned: Vec<String> = overrides.iter().map(|s| s.to_string()).collect();
    let parsed = ParamOverrides::parse(&owned).expect("valid overrides");
    identify_spec_file(&fixture(name), &parsed)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .identities
}

// ---------------------------------------------------------------------------
// SAME identity: irrelevant differences
// ---------------------------------------------------------------------------

#[test]
fn whitespace_key_order_and_comments_do_not_change_identity() {
    assert_eq!(ids("base.yaml"), ids("reformatted.yaml"));
}

#[test]
fn scalar_style_does_not_change_identity() {
    assert_eq!(ids("base.yaml"), ids("quoted.yaml"));
}

#[test]
fn human_id_and_meta_changes_do_not_change_identity() {
    assert_eq!(ids("base.yaml"), ids("id_meta_changed.yaml"));
}

#[test]
fn set_like_collections_reordered_and_duplicated_do_not_change_identity() {
    assert_eq!(ids("base.yaml"), ids("sets_reordered.yaml"));
}

#[test]
fn default_value_and_explicit_override_to_same_value_do_not_change_identity() {
    // base resolves helper_fixture from its default; the override supplies
    // exactly the same value, so the resolved experiment is identical.
    assert_eq!(
        ids("base.yaml"),
        ids_with(
            "base.yaml",
            &["helper_fixture=fixtures/existing_base64_helper.rs"]
        )
    );
}

#[test]
fn identity_is_deterministic_across_repeated_runs() {
    let first = ids("base.yaml");
    let second = ids("base.yaml");
    let third = ids("base.yaml");
    assert_eq!(first, second);
    assert_eq!(second, third);
}

// ---------------------------------------------------------------------------
// DIFFERENT identity: semantic differences
// ---------------------------------------------------------------------------

#[test]
fn attack_step_order_changes_identity() {
    let base = ids("base.yaml");
    let changed = ids("attack_order.yaml");
    assert_ne!(base.bat, changed.bat);
    assert_ne!(base.attack, changed.attack);
    // claim and oracle are untouched
    assert_eq!(base.claim, changed.claim);
    assert_eq!(base.oracle, changed.oracle);
}

#[test]
fn claim_change_changes_identity() {
    let base = ids("base.yaml");
    let changed = ids("claim_changed.yaml");
    assert_ne!(base.bat, changed.bat);
    assert_ne!(base.claim, changed.claim);
    // attack and oracle are untouched
    assert_eq!(base.attack, changed.attack);
    assert_eq!(base.oracle, changed.oracle);
}

#[test]
fn oracle_change_changes_identity() {
    let base = ids("base.yaml");
    let changed = ids("oracle_changed.yaml");
    assert_ne!(base.bat, changed.bat);
    assert_ne!(base.oracle, changed.oracle);
    assert_eq!(base.claim, changed.claim);
    assert_eq!(base.attack, changed.attack);
}

#[test]
fn timeout_change_changes_identity() {
    assert_ne!(ids("base.yaml").bat, ids("timeout_changed.yaml").bat);
}

#[test]
fn resolved_parameter_change_changes_identity() {
    let base = ids("base.yaml");
    let overridden = ids_with("base.yaml", &["helper_fixture=vendor/other_helper.rs"]);
    assert_ne!(base.bat, overridden.bat);
    assert_ne!(base.attack, overridden.attack);
    assert_eq!(base.claim, overridden.claim);
}

// ---------------------------------------------------------------------------
// Golden vectors: pinned canonical JSON and identities for base.yaml.
// If canonicalisation intentionally changes, these files must be updated
// through an explicit schema/identity-version decision — not casually.
// ---------------------------------------------------------------------------

#[test]
fn canonical_json_matches_golden_vector() {
    let identified = identify_spec_file(&fixture("base.yaml"), &ParamOverrides::default())
        .expect("base.yaml identifies");
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("base.canonical.json");
    let golden = std::fs::read_to_string(&golden_path).expect("golden canonical JSON exists");
    assert_eq!(
        identified.canonical_json.trim_end(),
        golden.trim_end(),
        "canonical JSON drifted from the golden vector; changing it requires \
         an explicit identity-version decision"
    );
}

#[test]
fn identities_match_golden_vector() {
    let ids = ids("base.yaml");
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("base.ids.txt");
    let golden = std::fs::read_to_string(&golden_path).expect("golden identities exist");
    let expected = format!(
        "bat     {}\nclaim   {}\nattack  {}\noracle  {}\n",
        ids.bat, ids.claim, ids.attack, ids.oracle
    );
    // Compare against the pinned values in the golden file.
    let golden_lines: Vec<&str> = golden.lines().collect();
    let actual_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(
        golden_lines, actual_lines,
        "identities drifted from the golden vector; changing them requires \
         an explicit identity-version decision"
    );
}

#[test]
fn identity_format_is_lowercase_hex_sha256() {
    let ids = ids("base.yaml");
    for (prefix, value) in [
        ("bat", &ids.bat),
        ("claim", &ids.claim),
        ("attack", &ids.attack),
        ("oracle", &ids.oracle),
    ] {
        let expected_prefix = format!("{prefix}:sha256:");
        assert!(value.starts_with(&expected_prefix), "{value}");
        let hex = &value[expected_prefix.len()..];
        assert_eq!(hex.len(), 64, "{value}");
        assert!(
            hex.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{value}"
        );
    }
}
