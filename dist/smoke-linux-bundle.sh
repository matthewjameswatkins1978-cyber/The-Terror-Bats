#!/bin/sh
# No-tag smoke for the Linux release path (tag-gate review, issue 1):
# package -> extract bundle -> install.sh from the bundle layout ->
# version -> doctor, plus a structural check that the aggregate job
# checks out the repo before rendering release notes (issue 2).
# Creates no tag, draft, or publication.
# Usage: dist/smoke-linux-bundle.sh [--repo ROOT] [--binary PATH] [--workflow NAME]
set -eu

REPO="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
BINARY=""
WORKFLOW="local"

while [ $# -gt 0 ]; do
    case "$1" in
        --repo) REPO="$2"; shift 2 ;;
        --binary) BINARY="$2"; shift 2 ;;
        --workflow) WORKFLOW="$2"; shift 2 ;;
        -h|--help) echo "usage: smoke-linux-bundle.sh [--repo ROOT] [--binary PATH] [--workflow NAME]" >&2; exit 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[ -n "$BINARY" ] || BINARY="$REPO/target/release/terrorbats"
[ -x "$BINARY" ] || { echo "error: binary not found: $BINARY (build first: cargo build --release --bin terrorbats)" >&2; exit 1; }

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT INT TERM

# 1. Package exactly as the tag workflow does.
sh "$REPO/dist/pack-linux.sh" --repo "$REPO" --binary "$BINARY" --out "$OUT" --workflow "$WORKFLOW"
TARBALL="$(ls "$OUT"/terrorbats-*-linux-x86_64.tar.gz | head -n 1)"
(cd "$OUT" && sha256sum -c "$(basename "$TARBALL").sha256")
echo "SMOKE-OK package: $TARBALL"

# 2. Install from the extracted bundle layout (never from the source tree:
# install.sh requires the executable beside it, which only the bundle has).
BUNDLE_DIR="$(mktemp -d)"
ROOT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT" "$BUNDLE_DIR" "$ROOT_DIR"' EXIT INT TERM
tar -xzf "$TARBALL" -C "$BUNDLE_DIR"
[ -x "$BUNDLE_DIR/install.sh" ] || { echo "error: install.sh missing from bundle" >&2; exit 1; }
sh "$BUNDLE_DIR/install.sh" --prefix "$ROOT_DIR"
BIN="$ROOT_DIR/bin/terrorbats"
[ -x "$BIN" ] || { echo "error: installed binary missing" >&2; exit 1; }
echo "SMOKE-OK install: $BIN"

# 3. Version and doctor against the installed copy.
"$BIN" --version
"$BIN" doctor
echo "SMOKE-OK version+doctor"

# 4. Structural: aggregate must check out the repo before the notes script.
python3 - "$REPO/.github/workflows/release.yml" <<'PYEOF'
import sys
path = sys.argv[1]
in_aggregate, checkout_at, notes_at = False, -1, -1
with open(path, encoding="utf-8") as f:
    for i, line in enumerate(f):
        if line.startswith("  aggregate:"):
            in_aggregate = True
        elif in_aggregate and line and not line[0].isspace():
            break
        if in_aggregate:
            if "actions/checkout" in line and checkout_at < 0:
                checkout_at = i
            if "release-notes.ps1" in line and notes_at < 0:
                notes_at = i
assert checkout_at >= 0, "aggregate job has no actions/checkout"
assert notes_at >= 0, "aggregate job does not render release-notes.ps1"
assert checkout_at < notes_at, "aggregate checkout must precede the notes renderer"
print("SMOKE-OK aggregate-checkout")
PYEOF
