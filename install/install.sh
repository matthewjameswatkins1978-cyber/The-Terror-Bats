#!/bin/sh
# The Terror Bats Framework 0.2.0-rc.1 — release installer (Linux).
# No sudo required. Never edits shell RC files. Finishes with
# `terrorbats --version` and `terrorbats doctor` as install proof.
set -eu

PREFIX="${HOME}/.local"
BINDIR=""
MAN=""
COMPLETIONS="yes"

usage() {
    echo "usage: install.sh [--prefix PREFIX] [--bindir DIR] [--no-completions]" >&2
    echo "  default: PREFIX=\$HOME/.local BINDIR=\$PREFIX/bin" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --bindir) BINDIR="$2"; shift 2 ;;
        --no-completions) COMPLETIONS="no"; shift ;;
        -h|--help) usage ;;
        *) usage ;;
    esac
done

[ -n "$BINDIR" ] || BINDIR="$PREFIX/bin"
MANDIR="$PREFIX/share/man/man1"
FISHDIR="$PREFIX/share/fish/vendor_completions.d"
BASHDIR="$PREFIX/share/bash-completion/completions"
ZSHDIR="$PREFIX/share/zsh/site-functions"

SRC_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
BINARY="$SRC_DIR/terrorbats"
[ -x "$BINARY" ] || { echo "error: terrorbats not found in '$SRC_DIR' (run from an extracted release bundle)" >&2; exit 1; }

mkdir -p "$BINDIR"
cp -f "$BINARY" "$BINDIR/terrorbats"
chmod 755 "$BINDIR/terrorbats"
echo "Installed: $BINDIR/terrorbats"

if [ "$COMPLETIONS" = "yes" ]; then
    mkdir -p "$MANDIR" "$FISHDIR" "$BASHDIR" "$ZSHDIR"
    if [ -f "$SRC_DIR/share/man/man1/terrorbats.1" ]; then
        cp -f "$SRC_DIR/share/man/man1/terrorbats.1" "$MANDIR/terrorbats.1"
        echo "Installed: $MANDIR/terrorbats.1"
    fi
    if [ -d "$SRC_DIR/completions" ]; then
        [ -f "$SRC_DIR/completions/terrorbats.bash" ] && cp -f "$SRC_DIR/completions/terrorbats.bash" "$BASHDIR/terrorbats" && echo "Installed: $BASHDIR/terrorbats"
        [ -f "$SRC_DIR/completions/terrorbats.zsh" ] && cp -f "$SRC_DIR/completions/terrorbats.zsh" "$ZSHDIR/_terrorbats" && echo "Installed: $ZSHDIR/_terrorbats"
        [ -f "$SRC_DIR/completions/terrorbats.fish" ] && cp -f "$SRC_DIR/completions/terrorbats.fish" "$FISHDIR/terrorbats.fish" && echo "Installed: $FISHDIR/terrorbats.fish"
        if [ -f "$SRC_DIR/completions/terrorbats.ps1" ]; then
            echo "Note: PowerShell completions ship in the bundle (completions/terrorbats.ps1) but are not installed on Linux; source it from your profile if wanted."
        fi
    fi
fi

"$BINDIR/terrorbats" --version

case ":$PATH:" in
    *":$BINDIR:"*) ;;
    *) echo "Note: '$BINDIR' is not on PATH. Add it, e.g.: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

"$BINDIR/terrorbats" doctor
