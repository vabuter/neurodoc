#!/usr/bin/env bash
# ============================================================
#  Neurodoc Editor - tag a release commit on main as vX.Y.Z
#  Linux twin of release.bat.
#
#  The tag is the single source of the version number: the Windows,
#  Linux and Android builds all read it via "git describe".
#  Nothing is pushed - publishing stays an explicit user action.
# ============================================================
set -u
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

VERSION="${1:-}"

if [ -z "$VERSION" ]; then
    echo "Usage:   ./release.sh X.Y.Z"
    echo "Example: ./release.sh 0.2.0"
    exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo
    echo "*** BAD VERSION: \"$VERSION\" ***"
    echo "Expected three numbers separated by dots, for example 0.2.0"
    exit 1
fi

# --- Releases are tagged on main. A tag on another branch would name a
# --- version that never reaches the released history.
BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null)"
if [ "$BRANCH" != "main" ]; then
    echo
    echo "*** WRONG BRANCH: \"$BRANCH\" ***"
    echo "Releases are tagged on main. Switch with: git switch main"
    exit 1
fi

# --- The tree must be clean: otherwise the tag points at a commit that does
# --- not match what will be built, and every build reports a "-dirty" version.
if [ -n "$(git status --porcelain)" ]; then
    echo
    echo "*** WORKING TREE IS NOT CLEAN ***"
    echo "Commit or stash your changes first, then run this script again."
    exit 1
fi

if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
    echo
    echo "*** TAG v$VERSION ALREADY EXISTS ***"
    echo "Tags are never moved. Pick a higher version number."
    exit 1
fi

if ! git tag -a "v$VERSION" -m "v$VERSION"; then
    echo
    echo "*** FAILED TO CREATE TAG ***"
    exit 1
fi

echo
echo "Tagged v$VERSION at:"
git --no-pager log -1 --oneline
echo
echo "Nothing has been pushed yet. To publish the release run:"
echo "    git push origin main --follow-tags"
echo
echo "Then rebuild to stamp the new version into the binaries:"
echo "    build-deploy.bat     (on Windows)"
echo "    ./linux-deploy.sh"
echo "    ./android-build.sh"
