//! Validation tests: malformed or ambiguous specs must be rejected with
//! human-understandable errors, never silently guessed.

use std::path::{Path, PathBuf};

use terrorbat::ParamOverrides;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Minimal valid spec used as the starting point for malformed variants.
const MINIMAL: &str = r#"
version: terrorbat/v1
id: minimal
claim:
  text: A falsifiable claim.
attack:
  run:
    - adapter: demo
      action: noop
oracle:
  all: []
evidence:
  capture:
    - stdout
"#;

fn parse_err(yaml: &str) -> String {
    let path = Path::new("<test>.yaml");
    let err = terrorbat::identify_spec_str(yaml, path, &ParamOverrides::default())
        .expect_err("spec must be rejected")
        .to_string();
    assert!(
        err.contains("<test>.yaml"),
        "error should name the file: {err}"
    );
    err
}

fn parse_err_with(yaml: &str, overrides: &[&str]) -> String {
    let owned: Vec<String> = overrides.iter().map(|s| s.to_string()).collect();
    let parsed = ParamOverrides::parse(&owned).expect("valid override syntax");
    let path = Path::new("<test>.yaml");
    terrorbat::identify_spec_str(yaml, path, &parsed)
        .expect_err("spec must be rejected")
        .to_string()
}

#[test]
fn minimal_valid_spec_is_accepted() {
    let path = Path::new("<test>.yaml");
    let identified =
        terrorbat::identify_spec_str(MINIMAL, path, &ParamOverrides::default()).expect("valid");
    assert_eq!(identified.human_id, "minimal");
    assert!(identified.identities.bat.starts_with("bat:sha256:"));
}

#[test]
fn missing_required_parameter_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: helper }
oracle: { all: [] }
evidence: { capture: [stdout] }
params:
  helper:
    type: string
    required: true
"#;
    let err = parse_err(yaml);
    assert!(
        err.contains("required parameter `helper` has no value"),
        "{err}"
    );
}

#[test]
fn undeclared_override_is_rejected() {
    let err = parse_err_with(MINIMAL, &["nope=1"]);
    assert!(err.contains("undeclared parameter `nope`"), "{err}");
}

#[test]
fn wrong_parameter_type_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: count }
oracle: { all: [] }
evidence: { capture: [stdout] }
params:
  count:
    type: integer
    default: 3
"#;
    let err = parse_err_with(yaml, &["count=not-a-number"]);
    assert!(err.contains("type integer"), "{err}");
    assert!(err.contains("not-a-number"), "{err}");
}

#[test]
fn wrong_default_type_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: flag }
oracle: { all: [] }
evidence: { capture: [stdout] }
params:
  flag:
    type: bool
    default: "yes"
"#;
    let err = parse_err(yaml);
    assert!(err.contains("type bool"), "{err}");
}

#[test]
fn param_reference_to_undeclared_name_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: ghost }
oracle: { all: [] }
evidence: { capture: [stdout] }
"#;
    let err = parse_err(yaml);
    assert!(err.contains("undeclared parameter"), "{err}");
}

#[test]
fn param_object_with_extra_keys_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: helper, fallback: something }
oracle: { all: [] }
evidence: { capture: [stdout] }
params:
  helper:
    type: string
    default: x
"#;
    let err = parse_err(yaml);
    assert!(err.contains("exactly one key"), "{err}");
}

#[test]
fn param_reference_with_non_string_name_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: p
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: { $param: 42 }
oracle: { all: [] }
evidence: { capture: [stdout] }
"#;
    let err = parse_err(yaml);
    assert!(
        err.contains("must be the name of a declared parameter"),
        "{err}"
    );
}

#[test]
fn unsupported_version_is_rejected() {
    let yaml = MINIMAL.replace("terrorbat/v1", "terrorbat/v2");
    let err = parse_err(&yaml);
    assert!(err.contains("unsupported Bat Spec version"), "{err}");
}

#[test]
fn unknown_top_level_field_is_rejected() {
    let yaml = format!("{MINIMAL}\nextra_section: nope\n");
    let err = parse_err(&yaml);
    assert!(err.contains("unknown field"), "{err}");
}

#[test]
fn unknown_field_in_typed_substructure_is_rejected() {
    let yaml = MINIMAL.replace(
        "claim:\n  text: A falsifiable claim.",
        "claim:\n  text: A falsifiable claim.\n  strength: high",
    );
    let err = parse_err(&yaml);
    assert!(err.contains("unknown field"), "{err}");
}

#[test]
fn duplicate_yaml_key_is_rejected() {
    let yaml = r#"
version: terrorbat/v1
id: dup
id: dup2
claim: { text: claim }
attack: { run: [] }
oracle: { all: [] }
evidence: { capture: [stdout] }
"#;
    let err = parse_err(yaml);
    assert!(err.contains("duplicate"), "{err}");
}

#[test]
fn multiple_yaml_documents_are_rejected() {
    let yaml = format!("{MINIMAL}\n---\nversion: terrorbat/v1\n");
    let err = parse_err(&yaml);
    assert!(err.contains("multiple YAML documents"), "{err}");
}

#[test]
fn invalid_timeout_formats_are_rejected() {
    for bad in ["10m", "0.5h", "600000ms", "60", "-5s", "s", ""] {
        let yaml = format!("{MINIMAL}\ntimeout:\n  run: \"{bad}\"\n");
        let err = parse_err(&yaml);
        assert!(err.contains("invalid timeout"), "for `{bad}`: {err}");
    }
}

#[test]
fn timeout_equivalent_representations_share_identity() {
    // "060s" and "60s" canonicalise to the same integer seconds.
    let a = format!("{MINIMAL}\ntimeout:\n  run: \"060s\"\n");
    let b = format!("{MINIMAL}\ntimeout:\n  run: \"60s\"\n");
    let path = Path::new("<test>.yaml");
    let ids_a = terrorbat::identify_spec_str(&a, path, &ParamOverrides::default())
        .expect("valid")
        .identities;
    let ids_b = terrorbat::identify_spec_str(&b, path, &ParamOverrides::default())
        .expect("valid")
        .identities;
    assert_eq!(ids_a, ids_b);
}

#[test]
fn wrong_typed_fields_are_rejected() {
    // requires must be strings
    let yaml = MINIMAL.replace(
        "claim:\n  text: A falsifiable claim.",
        "requires:\n  - 42\nclaim:\n  text: A falsifiable claim.",
    );
    let err = parse_err(&yaml);
    assert!(err.contains("schema violation"), "{err}");
    // oracle must be a mapping
    let yaml2 = MINIMAL.replace("oracle:\n  all: []", "oracle: just-a-string");
    let err2 = parse_err(&yaml2);
    assert!(err2.contains("oracle"), "{err2}");
}

#[test]
fn non_mapping_document_is_rejected() {
    let err = parse_err("- just\n- a\n- list\n");
    assert!(err.contains("YAML mapping"), "{err}");
}

#[test]
fn anchors_and_aliases_resolve_like_expanded_equivalents() {
    let yaml = r#"
version: terrorbat/v1
id: anchored
claim: { text: claim }
attack:
  run:
    - adapter: demo
      action: use
      value: &shared hello
    - adapter: demo
      action: use2
      value: *shared
oracle: { all: [] }
evidence: { capture: [stdout] }
"#;
    let expanded = yaml
        .replace("&shared hello", "hello")
        .replace("*shared", "hello");
    let path = Path::new("<test>.yaml");
    let a = terrorbat::identify_spec_str(yaml, path, &ParamOverrides::default())
        .expect("anchored valid");
    let b = terrorbat::identify_spec_str(&expanded, path, &ParamOverrides::default())
        .expect("expanded valid");
    assert_eq!(a.identities, b.identities);
}

#[test]
fn cli_override_duplicate_is_rejected() {
    let owned = vec!["a=1".to_string(), "a=2".to_string()];
    let err = ParamOverrides::parse(&owned).expect_err("duplicate override");
    assert!(err.to_string().contains("duplicate override"), "{err}");
}

#[test]
fn cli_override_malformed_is_rejected() {
    let owned = vec!["no-equals-sign".to_string()];
    let err = ParamOverrides::parse(&owned).expect_err("malformed override");
    assert!(err.to_string().contains("expected `name=value`"), "{err}");
}

#[test]
fn missing_file_reports_readable_error() {
    let err = terrorbat::identify_spec_file(
        &fixture_dir().join("does_not_exist.yaml"),
        &ParamOverrides::default(),
    )
    .expect_err("missing file")
    .to_string();
    assert!(err.contains("cannot read"), "{err}");
}

#[test]
fn fixture_files_all_parse() {
    // Every checked-in fixture must be individually valid (except files
    // deliberately named invalid_*).
    for entry in std::fs::read_dir(fixture_dir()).expect("fixtures dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "yaml")
            && !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("invalid_")
        {
            terrorbat::identify_spec_file(&path, &ParamOverrides::default())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
