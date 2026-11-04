#!/usr/bin/env bash
# ============================================
#  Neurodoc Editor for Android - debug build
#  Linux twin of android-build.bat. Unlike the
#  bat file it never waits for a key press, so
#  it is safe to call from automation.
# ============================================
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT/android" || exit 1

# Gradle looks for the SDK in local.properties (Android Studio writes it) and
# then in ANDROID_HOME / ANDROID_SDK_ROOT. A fresh clone has neither, so fall
# back to ~/.local/opt/android-sdk (the home of hand-installed software).
if ! grep -qs '^sdk\.dir=' local.properties && [ -z "${ANDROID_HOME:-}" ] && [ -z "${ANDROID_SDK_ROOT:-}" ]; then
    export ANDROID_HOME="$HOME/.local/opt/android-sdk"
fi

if ! ./gradlew assembleDebug; then
    echo
    echo "*** BUILD FAILED ***"
    exit 1
fi

echo
echo "Build succeeded. APK:"
echo "$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
