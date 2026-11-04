#!/usr/bin/env bash
# ==========================================================
#  Neurodoc Editor for Linux - release build + install for
#  the current user (no root): the program lives in its own
#  folder ~/.local/opt/neurodoc-editor, ~/.local/bin gets a
#  symlink to it, the launcher and icons go to ~/.local/share
#  (XDG requires that), so the app shows up in the GNOME app grid.
#  Linux twin of build-deploy.bat.
# ==========================================================

# Target folders (change if needed):
OPT_DIR="$HOME/.local/opt/neurodoc-editor"
BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share"
APP_ID="today.vab.NeurodocEditor"

set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Guard: a running instance keeps the old executable busy; installing over it
# --- would leave the user with a half-updated program until the next start.
if pgrep -x neurodoc-editor >/dev/null 2>&1; then
    echo
    echo "*** neurodoc-editor IS RUNNING ***"
    echo "Close the application and run this script again."
    exit 1
fi

"$ROOT/linux-build.sh" || exit 1

EXE="$ROOT/linux/target/release/neurodoc-editor"
mkdir -p "$OPT_DIR" "$BIN_DIR" "$SHARE_DIR/applications"
install -m755 "$EXE" "$OPT_DIR/neurodoc-editor" || {
    echo
    echo "*** INSTALL FAILED ***"
    exit 1
}

# --- Symlink in PATH; a plain file left by an older deploy is replaced
ln -sfn "$OPT_DIR/neurodoc-editor" "$BIN_DIR/neurodoc-editor"

# --- Launcher: the file name must equal the application id, GNOME finds the
# --- window's icon by it on Wayland.
sed "s|@EXEC@|$BIN_DIR/neurodoc-editor|" "$ROOT/linux/data/$APP_ID.desktop.in" \
    > "$SHARE_DIR/applications/$APP_ID.desktop"

# --- Icons from the shared assets of the Windows project (hicolor theme)
for size in 32 64 128 256 512; do
    install -Dm644 "$ROOT/windows/NeurodocEditor/Assets/app-icon-$size.png" \
        "$SHARE_DIR/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$SHARE_DIR/applications" 2>/dev/null
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$SHARE_DIR/icons/hicolor" 2>/dev/null

echo
echo "Deploy finished. Run the app from the application grid or:"
echo "$BIN_DIR/neurodoc-editor"
echo
echo "To uninstall:"
echo "    rm -rf $OPT_DIR; rm $BIN_DIR/neurodoc-editor $SHARE_DIR/applications/$APP_ID.desktop $SHARE_DIR/icons/hicolor/*/apps/$APP_ID.png"
