#!/bin/sh
# The Terror Bats Framework 0.2.0-rc.1 — uninstaller (Linux).
# Removes exactly what install.sh owns. Never touches Bat files,
# evidence stores, repositories, receipts, or shell RC files.
set -eu

PREFIX="${HOME}/.local"
BINDIR=""

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --bindir) BINDIR="$2"; shift 2 ;;
        -h|--help) echo "usage: uninstall.sh [--prefix PREFIX] [--bindir DIR]" >&2; exit 2 ;;
        *) echo "usage: uninstall.sh [--prefix PREFIX] [--bindir DIR]" >&2; exit 2 ;;
    esac
done

[ -n "$BINDIR" ] || BINDIR="$PREFIX/bin"
REMOVED=""

remove_file() {
    if [ -e "$1" ]; then
        rm -f "$1"
        REMOVED="$REMOVED
  $1"
    fi
}

remove_file "$BINDIR/terrorbats"
remove_file "$PREFIX/share/man/man1/terrorbats.1"
remove_file "$PREFIX/share/bash-completion/completions/terrorbats"
remove_file "$PREFIX/share/zsh/site-functions/_terrorbats"
remove_file "$PREFIX/share/fish/vendor_completions.d/terrorbats.fish"

if [ -z "$REMOVED" ]; then
    echo "Nothing owned by the installer was found under '$PREFIX'."
else
    echo "Removed:$REMOVED"
fi
echo "Untouched by design: Bat files, evidence stores, repositories, receipts, shell RC files."
