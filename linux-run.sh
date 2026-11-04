#!/usr/bin/env bash
# ============================================
#  Neurodoc Editor for Linux - build and run
#  Usage: ./linux-run.sh [--debug] [app args]
#  For an isolated instance (own settings, so the
#  real projects stay untouched) run for example:
#    XDG_CONFIG_HOME=/tmp/nd/config ./linux-run.sh
# ============================================
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

PROFILE=release
BUILD_FLAG=()
if [ "${1:-}" = "--debug" ]; then
    PROFILE=debug
    BUILD_FLAG=(--debug)
    shift
fi

"$ROOT/linux-build.sh" "${BUILD_FLAG[@]}" || exit 1

EXE="$ROOT/linux/target/$PROFILE/neurodoc-editor"
exec "$EXE" "$@"
