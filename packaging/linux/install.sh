#!/bin/sh
# Install Plum for the current user (default) or system-wide:
#
#   ./install.sh                     # ~/.local
#   sudo ./install.sh --prefix /usr/local
#   ./install.sh --uninstall [--prefix DIR]
#
# Works from a source checkout (after `cargo build --release`) and from an
# extracted release tarball (binary next to this script).
set -eu

prefix="$HOME/.local"
uninstall=0
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) prefix="$2"; shift 2 ;;
        --prefix=*) prefix="${1#--prefix=}"; shift ;;
        --uninstall) uninstall=1; shift ;;
        -h|--help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

here="$(cd "$(dirname "$0")" && pwd)"
bin="$prefix/bin/plum"
apps="$prefix/share/applications"
mime="$prefix/share/mime"
icons="$prefix/share/icons/hicolor"

refresh() {
    command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$mime" >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" >/dev/null 2>&1 || true
}

if [ "$uninstall" = 1 ]; then
    rm -f "$bin" "$apps/plum.desktop" "$mime/packages/plum.xml" \
        "$icons/scalable/apps/plum.svg" "$icons/256x256/apps/plum.png"
    refresh
    echo "Plum removed from $prefix"
    exit 0
fi

# Locate the pieces: release tarball layout first, then a source checkout.
find_file() {
    for f in "$@"; do [ -f "$f" ] && { echo "$f"; return 0; }; done
    return 1
}
src_bin="$(find_file "$here/plum" "$here/../../target/release/plum")" || {
    echo "plum binary not found; run 'cargo build --release' first" >&2; exit 1; }
svg="$(find_file "$here/plum.svg" "$here/../../crates/plum-app/assets/icon.svg")"
png="$(find_file "$here/plum.png" "$here/../../crates/plum-app/assets/icon.png")"

install -Dm755 "$src_bin" "$bin"
install -Dm644 "$here/plum.desktop" "$apps/plum.desktop"
install -Dm644 "$here/plum.xml" "$mime/packages/plum.xml"
install -Dm644 "$svg" "$icons/scalable/apps/plum.svg"
install -Dm644 "$png" "$icons/256x256/apps/plum.png"
refresh

echo "Plum installed to $bin"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "note: $prefix/bin is not on your PATH" ;;
esac
