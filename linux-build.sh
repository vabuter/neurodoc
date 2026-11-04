#!/usr/bin/env bash
# ============================================
#  Neurodoc Editor for Linux - release build
#  Rust + GTK4/libadwaita + WebKitGTK. The version
#  comes from the git tag (linux/build.rs), exactly
#  as for the Windows and Android builds.
#  Usage: ./linux-build.sh [--debug]
# ============================================
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT/linux" || exit 1

PROFILE=release
CARGO_FLAGS=(--release)
if [ "${1:-}" = "--debug" ]; then
    PROFILE=debug
    CARGO_FLAGS=()
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "*** cargo NOT FOUND ***"
    echo "Install the toolchain and the GTK development packages (Fedora):"
    echo "    sudo dnf install rust cargo rustfmt clippy gtk4-devel libadwaita-devel webkitgtk6.0-devel glib2-devel"
    exit 1
fi

for lib in gtk4 libadwaita-1 webkitgtk-6.0; do
    if ! pkg-config --exists "$lib" 2>/dev/null; then
        echo "*** $lib DEVELOPMENT PACKAGE NOT FOUND ***"
        echo "Fedora: sudo dnf install gtk4-devel libadwaita-devel webkitgtk6.0-devel"
        echo "Debian/Ubuntu: sudo apt install libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev"
        exit 1
    fi
done

if ! cargo build "${CARGO_FLAGS[@]}"; then
    echo
    echo "*** BUILD FAILED ***"
    exit 1
fi

echo
echo "Build succeeded. Executable:"
echo "$ROOT/linux/target/$PROFILE/neurodoc-editor"
