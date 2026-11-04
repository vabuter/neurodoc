#!/usr/bin/env bash
# ============================================
#  Neurodoc Editor - screenshots for the book
#  Usage: ./book-shots.sh
#  Runs the Linux self-check (NEURODOC_SMOKE) on a copy of example/
#  inside a headless GNOME compositor and writes the shots the book
#  uses to book/src/images/*.webp (lossless).
#  Needs: mutter, dbus-run-session, ImageMagick 7 (magick), git.
# ============================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGES="$ROOT/book/src/images"

for tool in mutter dbus-run-session magick git; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Not found: $tool"
        exit 1
    fi
done

# The window title shows the version: build from a clean tagged commit
# to get a plain X.Y.Z there.
VERSION="$(git -C "$ROOT" describe --tags --dirty --always --match 'v[0-9]*' 2>/dev/null || true)"
case "$VERSION" in
    *-dirty) echo "Warning: uncommitted changes, the window title will show $VERSION" ;;
esac

"$ROOT/linux-build.sh" || exit 1
EXE="$ROOT/linux/target/release/neurodoc-editor"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
CATALOG="$WORK/Fikus"
mkdir -p "$WORK/shots" "$WORK/out"
cp -a "$ROOT/example" "$CATALOG"

# --- A git history for the history card and the diff: three commits,
#     each adds one more of the last non-empty lines of root.
#     Dates are in UTC and the app runs with TZ=UTC, so the times in
#     the card match the commit messages.
cp "$CATALOG/root" "$WORK/root.full"
without_last() {  # without_last N < file: drop the last N non-empty lines
    awk -v drop="$1" '{ line[NR] = $0; if (length($0)) ne[++n] = NR }
        END { for (j = n - drop + 1; j <= n; j++) skip[ne[j]] = 1
              for (i = 1; i <= NR; i++) if (!(i in skip)) print line[i] }'
}
commit() {  # commit <date> <author> <email> <message>
    GIT_AUTHOR_DATE="$1" GIT_COMMITTER_DATE="$1" \
        git -C "$CATALOG" -c user.name="$2" -c user.email="$3" commit -q -am "$4"
}
git -C "$CATALOG" init -q -b main
git -C "$CATALOG" config core.autocrlf false
without_last 2 < "$WORK/root.full" > "$CATALOG/root"
git -C "$CATALOG" add -A
commit "2026-10-27T10:05:00Z" "Анна" "anna@fikus.example" "2026-10-27 10:05 · laptop"
without_last 1 < "$WORK/root.full" > "$CATALOG/root"
commit "2026-10-29T19:42:00Z" "Игорь" "igor@fikus.example" "2026-10-29 19:42 · phone"
cp "$WORK/root.full" "$CATALOG/root"
commit "2026-10-31T08:15:00Z" "Анна" "anna@fikus.example" "2026-10-31 08:15 · laptop"

# --- Settings of an isolated instance. The project path is relative
#     (the app runs from $WORK): the projects dialog on the shot shows
#     "Fikus" instead of a long temporary path. The disk watcher gets
#     absolute paths and then takes own autosaves for foreign edits, so
#     the "smoke: ..." verdicts of this run do not count - check those
#     as described in book/src/dev/testing.md.
settings() {  # settings <theme>
    mkdir -p "$WORK/config/neurodoc-editor"
    cat > "$WORK/config/neurodoc-editor/settings.json" <<EOF
{"theme": "$1", "fontFamily": "JetBrains Mono", "fontSize": 14.0, "lineSpacing": 1.0,
 "rawEditing": false, "currentProjectId": "fikus",
 "projects": [{"id": "fikus", "name": "Fikus", "path": "Fikus", "svgBackground": ""}]}
EOF
}
headless() {  # headless <env...>: the app in its own D-Bus session and compositor
    (cd "$WORK" && env TZ=UTC XDG_CONFIG_HOME="$WORK/config" "$@" \
        timeout 300 dbus-run-session -- \
        mutter --headless --wayland --no-x11 --virtual-monitor 1600x1000 \
        --wayland-display "neurodoc-shots-$$" -- "$EXE" 2>> "$WORK/smoke.log") || true
}

echo "Self-check run..."
settings Dark
headless NEURODOC_SMOKE="$WORK/shots"
# Switching the theme on the fly is not caught by a headless snapshot,
# so the light window is a separate start with the light theme.
echo "Light theme..."
settings Light
headless NEURODOC_SNAPSHOT="$WORK/light.png" NEURODOC_SNAPSHOT_QUIT=1

# --- Self-check step -> image name in the book
MAP="01-root:main-window 02-linked-file:linked-file 03-yaml:viewer-yaml
04-md:viewer-md 05a-svg:viewer-svg 05b-raw:raw-mode 06-find:find
07-search:project-search 08-broken:broken-links 09-settings:settings
10-hotkeys:hotkeys 11-open:open-by-id 12-projects:projects
15-git-history:git-history 17-git-diff:git-diff"
failed=0
place() {  # place <source png> <name>: into $WORK/out first, all or nothing
    if [ ! -s "$1" ]; then
        echo "Missing shot: $(basename "$1")"
        failed=1
        return
    fi
    magick "$1" -strip -define webp:lossless=true -define webp:method=6 "$WORK/out/$2.webp"
    echo "    $2.webp"
}
for pair in $MAP; do
    place "$WORK/shots/${pair%%:*}.png" "${pair##*:}"
done
place "$WORK/light.png" main-window-light

if [ $failed -ne 0 ]; then
    echo "Some shots are missing, $IMAGES is left as it was. Log:"
    grep -E '^smoke: снимок' "$WORK/smoke.log" || tail -20 "$WORK/smoke.log"
    exit 1
fi
mkdir -p "$IMAGES"
cp "$WORK/out/"*.webp "$IMAGES/"
echo "Done: $IMAGES"
