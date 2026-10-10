//! Dry run of the generated tag-release notes: the renderer must expand
//! only the version title while backticks, `$HOME`, and shell examples
//! survive byte-for-byte (an unquoted shell heredoc would execute the
//! `code spans` as commands). Runs everywhere `pwsh` exists, including
//! the Ubuntu aggregate release lane; skips with notice where it does not.
use std::process::Command;

fn render(version: &str) -> Option<String> {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("dist")
        .join("release-notes.ps1");
    let probe = Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("'x'")
        .output();
    if probe.is_err() {
        eprintln!("SKIP: pwsh not available; release-notes dry run needs it");
        return None;
    }
    let out = Command::new("pwsh")
        .args([
            "-NoProfile",
            "-File",
            &script.display().to_string(),
            "-Version",
            version,
        ])
        .output()
        .expect("pwsh runs");
    assert!(
        out.status.success(),
        "release-notes render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[test]
fn release_notes_survive_verbatim_with_single_interpolation() {
    let Some(out) = render("9.9.9-dryrun") else {
        return;
    };
    // Single controlled interpolation: the probe version appears once (title).
    assert_eq!(
        out.matches("9.9.9-dryrun").count(),
        1,
        "version must interpolate exactly once:\n{out}"
    );
    assert!(
        out.starts_with("# The Terror Bats Framework 9.9.9-dryrun"),
        "title line must carry the version:\n{out}"
    );
    // Backtick code spans and shell examples survive literally.
    for span in [
        "`install.ps1`",
        "`terrorbats --version`",
        "`terrorbats doctor`",
        "`Get-FileHash -Algorithm SHA256`",
        "`sha256sum -c SHA256SUMS.txt`",
        "`process`",
        "`install.sh --prefix $HOME/.local`",
        "$HOME/.local",
        "PROVEN means the tested claim was falsified under recorded conditions.",
    ] {
        assert!(
            out.contains(span),
            "notes must contain {span} verbatim:\n{out}"
        );
    }
    // No command-substitution damage.
    assert!(
        !out.contains("command not found"),
        "notes must not embed shell errors:\n{out}"
    );
}
