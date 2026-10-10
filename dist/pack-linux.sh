#!/bin/sh
# Assemble the Linux RC1 bundle (assembly only; gates and the release
# build run separately in the release workflow).
# Usage: dist/pack-linux.sh [--repo ROOT] [--binary PATH] [--out DIR] [--workflow NAME]
set -eu

REPO="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
BINARY=""
OUTDIR="$REPO/dist"
VERSION=""
WORKFLOW="local"

while [ $# -gt 0 ]; do
    case "$1" in
        --repo) REPO="$2"; shift 2 ;;
        --binary) BINARY="$2"; shift 2 ;;
        --out) OUTDIR="$2"; shift 2 ;;
        --workflow) WORKFLOW="$2"; shift 2 ;;
        -h|--help) echo "usage: pack-linux.sh [--repo ROOT] [--binary PATH] [--out DIR] [--workflow NAME]" >&2; exit 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[ -n "$BINARY" ] || BINARY="$REPO/target/release/terrorbats"
[ -x "$BINARY" ] || { echo "error: binary not found: $BINARY (build first: cargo build --release --bin terrorbats)" >&2; exit 1; }
if [ -z "$VERSION" ]; then
    VERSION="$("$BINARY" --version | sed 's/^terrorbats //')"
    [ -n "$VERSION" ] || { echo "error: could not determine version" >&2; exit 1; }
fi
COMMIT="$(git -C "$REPO" rev-parse HEAD)"
RUSTC="$(rustc --version)"
ARTIFACT="terrorbats-$VERSION-linux-x86_64"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT INT TERM
mkdir -p "$STAGE/completions" "$STAGE/share/man/man1"

cp "$BINARY" "$STAGE/terrorbats"
chmod 755 "$STAGE/terrorbats"
for doc in README.md CHANGELOG.md LICENSE-MIT LICENSE-APACHE; do
    cp "$REPO/$doc" "$STAGE/"
done
cp "$REPO/docs/MANUAL.md" "$STAGE/MANUAL.md"
cp "$REPO/install/install.sh" "$REPO/install/uninstall.sh" "$STAGE/"

"$STAGE/terrorbats" completions bash > "$STAGE/completions/terrorbats.bash"
"$STAGE/terrorbats" completions zsh > "$STAGE/completions/terrorbats.zsh"
"$STAGE/terrorbats" completions fish > "$STAGE/completions/terrorbats.fish"
"$STAGE/terrorbats" completions powershell > "$STAGE/completions/terrorbats.ps1"
"$STAGE/terrorbats" man > "$STAGE/share/man/man1/terrorbats.1"

if [ -d "$REPO/assets/brand" ]; then
    mkdir -p "$STAGE/assets"
    cp -r "$REPO/assets/brand" "$STAGE/assets/brand"
else
    echo "NOTE: assets/brand/ not present; bundle ships without brand assets."
fi

cat > "$STAGE/BUILDINFO.json" <<EOF
{
  "product": "The Terror Bats Framework",
  "version": "$VERSION",
  "git_commit": "$COMMIT",
  "target": "x86_64-unknown-linux-gnu",
  "rust_version": "$RUSTC",
  "build_profile": "release",
  "build_workflow": "$WORKFLOW",
  "artifact_name": "$ARTIFACT.tar.gz"
}
EOF

mkdir -p "$OUTDIR"
tar -czf "$OUTDIR/$ARTIFACT.tar.gz" -C "$STAGE" .
(cd "$OUTDIR" && sha256sum "$ARTIFACT.tar.gz" > "$ARTIFACT.tar.gz.sha256")
echo "Bundle: $OUTDIR/$ARTIFACT.tar.gz"
cat "$OUTDIR/$ARTIFACT.tar.gz.sha256"
