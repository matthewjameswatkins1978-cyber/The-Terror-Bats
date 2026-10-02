//! Bat Pack v1 tests: strict manifests, effective-Bat resolution, and the
//! `pack:sha256:` identity contract (metadata/formatting-stable,
//! semantic-sensitive).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use terrorbat::pack::{identify_pack_file, pack_identity};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Minimal valid Bat Spec (no params).
const BAT_A: &str = r#"
version: terrorbat/v1
id: bat-a
claim:
  text: Claim A is falsifiable.
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

/// Second minimal Bat Spec with different semantics.
const BAT_B: &str = r#"
version: terrorbat/v1
id: bat-b
claim:
  text: Claim B is falsifiable and different.
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

/// Bat Spec with a string param (default `hello`).
const BAT_P: &str = r#"
version: terrorbat/v1
id: bat-p
claim:
  text: Claim P is falsifiable.
attack:
  run:
    - adapter: demo
      action: use
      value:
        $param: greeting
oracle:
  all: []
evidence:
  capture:
    - stdout
params:
  greeting:
    type: string
    default: hello
"#;

fn scratch_dir(test: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "terrorbat-pack-test-{}-{}-{}",
        std::process::id(),
        test,
        n
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn pack_path(dir: &Path) -> PathBuf {
    dir.join("pack.yaml")
}

fn identify(dir: &Path) -> terrorbat::pack::IdentifiedPack {
    identify_pack_file(&pack_path(dir)).unwrap_or_else(|e| panic!("pack identifies: {e}"))
}

fn identify_err(dir: &Path) -> String {
    identify_pack_file(&pack_path(dir))
        .expect_err("pack must be rejected")
        .to_string()
}

fn two_bat_pack() -> (PathBuf, PathBuf) {
    let dir = scratch_dir("valid");
    write(&dir, "a.yaml", BAT_A);
    write(&dir, "b.yaml", BAT_B);
    let pack = dir.join("pack.yaml");
    std::fs::write(
        &pack,
        "version: terrorbat-pack/v1\nid: demo-pack\nbats:\n  - path: a.yaml\n  - path: b.yaml\n",
    )
    .expect("write fixture");
    (dir, pack)
}

// ---------------------------------------------------------------------------
// Valid packs
// ---------------------------------------------------------------------------

#[test]
fn valid_pack_identifies_with_ordered_entries() {
    let (dir, _) = two_bat_pack();
    let pack = identify(&dir);
    assert_eq!(pack.human_id, "demo-pack");
    assert!(
        pack.identity.starts_with("pack:sha256:"),
        "{}",
        pack.identity
    );
    assert_eq!(pack.entries.len(), 2);
    assert_eq!(pack.entries[0].path, "a.yaml");
    assert_eq!(pack.entries[1].path, "b.yaml");
    for entry in &pack.entries {
        assert!(entry.bat.starts_with("bat:sha256:"), "{}", entry.bat);
    }
    // Effective bat identities match direct identification of the bat files.
    let direct_a =
        terrorbat::identify_spec_file(&dir.join("a.yaml"), &terrorbat::ParamOverrides::default())
            .expect("bat a identifies");
    assert_eq!(pack.entries[0].bat, direct_a.identities.bat);
}

#[test]
fn pack_params_flow_into_effective_bat_identity() {
    let dir = scratch_dir("params");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: param-pack\nbats:\n  - path: p.yaml\n    params:\n      greeting: overridden\n",
    );
    let pack = identify(&dir);
    let expected = terrorbat::identify_spec_file(
        &dir.join("p.yaml"),
        &terrorbat::ParamOverrides::parse(&["greeting=overridden".to_string()]).expect("overrides"),
    )
    .expect("bat p identifies");
    assert_eq!(pack.entries[0].bat, expected.identities.bat);
    // And the override actually changes the effective bat.
    let plain =
        terrorbat::identify_spec_file(&dir.join("p.yaml"), &terrorbat::ParamOverrides::default())
            .expect("bat p identifies");
    assert_ne!(pack.entries[0].bat, plain.identities.bat);
}

#[test]
fn pack_scalar_params_cover_bool_and_integer() {
    let dir = scratch_dir("scalars");
    write(
        &dir,
        "p.yaml",
        r#"
version: terrorbat/v1
id: typed
claim:
  text: Typed params.
attack:
  run:
    - adapter: demo
      action: use
      flag:
        $param: flag
      count:
        $param: count
oracle:
  all: []
evidence:
  capture:
    - stdout
params:
  flag:
    type: bool
    default: false
  count:
    type: integer
    default: 0
"#,
    );
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: scalar-pack\nbats:\n  - path: p.yaml\n    params:\n      flag: true\n      count: 42\n",
    );
    let pack = identify(&dir);
    assert!(pack.identity.starts_with("pack:sha256:"));
}

// ---------------------------------------------------------------------------
// Rejections
// ---------------------------------------------------------------------------

#[test]
fn invalid_version_is_rejected() {
    let dir = scratch_dir("version");
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat/v1\nid: wrong-version\nbats:\n  - path: a.yaml\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("terrorbat-pack/v1"), "{err}");
}

#[test]
fn unknown_field_is_rejected() {
    let dir = scratch_dir("unknown");
    write(&dir, "a.yaml", BAT_A);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: a.yaml\ncampaign: someday\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("campaign"), "{err}");
}

#[test]
fn unknown_entry_field_is_rejected() {
    let dir = scratch_dir("entry-field");
    write(&dir, "a.yaml", BAT_A);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: a.yaml\n    retries: 3\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("retries"), "{err}");
}

#[test]
fn empty_bats_is_rejected() {
    let dir = scratch_dir("empty");
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats: []\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("non-empty"), "{err}");
}

#[test]
fn missing_bat_is_rejected() {
    let dir = scratch_dir("missing");
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: nope.yaml\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("nope.yaml"), "{err}");
}

#[test]
fn invalid_bat_is_rejected() {
    let dir = scratch_dir("badbat");
    write(&dir, "bad.yaml", "version: terrorbat/v1\nid: broken\n");
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: bad.yaml\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("bad.yaml"), "{err}");
}

#[test]
fn float_param_is_rejected() {
    let dir = scratch_dir("float");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: p.yaml\n    params:\n      greeting: 1.5\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("greeting"), "{err}");
}

#[test]
fn null_param_is_rejected() {
    let dir = scratch_dir("null");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: p.yaml\n    params:\n      greeting: null\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("greeting"), "{err}");
}

#[test]
fn array_param_is_rejected() {
    let dir = scratch_dir("array");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: p.yaml\n    params:\n      greeting: [a, b]\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("greeting"), "{err}");
}

#[test]
fn object_param_is_rejected() {
    let dir = scratch_dir("object");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: p.yaml\n    params:\n      greeting: {a: b}\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("greeting"), "{err}");
}

#[test]
fn unsafe_integer_param_is_rejected() {
    let dir = scratch_dir("unsafe");
    write(
        &dir,
        "p.yaml",
        r#"
version: terrorbat/v1
id: inty
claim:
  text: Int param.
attack:
  run:
    - adapter: demo
      action: use
      count:
        $param: count
oracle:
  all: []
evidence:
  capture:
    - stdout
params:
  count:
    type: integer
    default: 0
"#,
    );
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: x\nbats:\n  - path: p.yaml\n    params:\n      count: 9007199254740992\n",
    );
    let err = identify_err(&dir);
    assert!(err.contains("9007199254740992"), "{err}");
    assert!(err.contains("safe"), "{err}");
}

// ---------------------------------------------------------------------------
// Identity contract
// ---------------------------------------------------------------------------

#[test]
fn identity_is_deterministic() {
    let (dir, _) = two_bat_pack();
    let first = identify(&dir);
    let second = identify_pack_file(&pack_path(&dir)).expect("re-identify");
    assert_eq!(first.identity, second.identity);
    assert_eq!(first.canonical_json, second.canonical_json);
}

#[test]
fn identity_format_is_lowercase_hex_sha256() {
    let (dir, _) = two_bat_pack();
    let pack = identify(&dir);
    let hex = pack
        .identity
        .strip_prefix("pack:sha256:")
        .expect("pack identity prefix");
    assert_eq!(hex.len(), 64, "{}", pack.identity);
    assert!(
        hex.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{}",
        pack.identity
    );
}

#[test]
fn formatting_only_change_leaves_identity() {
    let dir = scratch_dir("format");
    write(&dir, "a.yaml", BAT_A);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: demo-pack\nbats:\n  - path: a.yaml\n",
    );
    let before = identify(&dir);
    write(
        &dir,
        "pack.yaml",
        "# a comment\nversion:   terrorbat-pack/v1\nid: demo-pack\nbats:\n\n  - path: a.yaml\n",
    );
    let after = identify(&dir);
    assert_eq!(before.identity, after.identity);
}

#[test]
fn metadata_only_change_leaves_identity() {
    let dir = scratch_dir("meta");
    write(&dir, "a.yaml", BAT_A);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: first-label\nbats:\n  - path: a.yaml\n",
    );
    let before = identify(&dir);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: second-label\ndescription: New human words.\nbats:\n  - path: a.yaml\n",
    );
    let after = identify(&dir);
    assert_eq!(before.identity, after.identity);
    assert_ne!(before.human_id, after.human_id);
}

#[test]
fn bat_semantic_change_alters_identity() {
    let dir = scratch_dir("semantic");
    write(&dir, "a.yaml", BAT_A);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: a.yaml\n",
    );
    let before = identify(&dir);
    write(&dir, "a.yaml", BAT_B);
    let after = identify(&dir);
    assert_ne!(before.identity, after.identity);
}

#[test]
fn entry_order_change_alters_identity() {
    let dir = scratch_dir("order");
    write(&dir, "a.yaml", BAT_A);
    write(&dir, "b.yaml", BAT_B);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: a.yaml\n  - path: b.yaml\n",
    );
    let before = identify(&dir);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: b.yaml\n  - path: a.yaml\n",
    );
    let after = identify(&dir);
    assert_ne!(before.identity, after.identity);
}

#[test]
fn param_change_alters_identity() {
    let dir = scratch_dir("paramchange");
    write(&dir, "p.yaml", BAT_P);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: p.yaml\n    params:\n      greeting: one\n",
    );
    let before = identify(&dir);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: p.yaml\n    params:\n      greeting: two\n",
    );
    let after = identify(&dir);
    assert_ne!(before.identity, after.identity);
}

#[test]
fn membership_change_alters_identity() {
    let dir = scratch_dir("member");
    write(&dir, "a.yaml", BAT_A);
    write(&dir, "b.yaml", BAT_B);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: a.yaml\n  - path: b.yaml\n",
    );
    let before = identify(&dir);
    write(
        &dir,
        "pack.yaml",
        "version: terrorbat-pack/v1\nid: p\nbats:\n  - path: a.yaml\n",
    );
    let after = identify(&dir);
    assert_ne!(before.identity, after.identity);
}

#[test]
fn identity_covers_only_version_and_effective_bats() {
    let (dir, _) = two_bat_pack();
    let pack = identify(&dir);
    let expected_ids: Vec<String> = pack.entries.iter().map(|e| e.bat.clone()).collect();
    let (canonical_json, identity) = pack_identity(&expected_ids).expect("pack identity");
    assert_eq!(pack.identity, identity);
    assert_eq!(pack.canonical_json, canonical_json);
    // The canonical projection names exactly {bats, version} and nothing else.
    let parsed: serde_json::Value =
        serde_json::from_str(&pack.canonical_json).expect("canonical JSON parses");
    let obj = parsed.as_object().expect("projection is a mapping");
    assert_eq!(
        obj.keys().collect::<Vec<_>>(),
        ["bats".to_string(), "version".to_string()]
            .iter()
            .collect::<Vec<_>>(),
        "projection must contain exactly version + bats"
    );
}
