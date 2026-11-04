#!/usr/bin/env bash
# =====================================================
#  Neurodoc Editor for Android - build and install
#  Linux twin of android-install.bat.
#  Requires a device connected over USB (adb devices);
#  with several devices pick one via ANDROID_SERIAL.
# =====================================================
set -u

PACKAGE="today.vab.neurodoceditor"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT/android" || exit 1

# The SDK is searched in the same order as Gradle does: local.properties,
# ANDROID_HOME, ANDROID_SDK_ROOT - and then ~/.local/opt/android-sdk.
SDK_DIR="$(sed -n 's/^sdk\.dir=//p' local.properties 2>/dev/null | tail -n 1 | sed 's/\\\(.\)/\1/g')"
if [ -z "$SDK_DIR" ] && [ -z "${ANDROID_HOME:-}" ] && [ -z "${ANDROID_SDK_ROOT:-}" ]; then
    export ANDROID_HOME="$HOME/.local/opt/android-sdk"
fi
SDK_DIR="${SDK_DIR:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"

if ! ./gradlew assembleDebug; then
    echo
    echo "*** BUILD FAILED ***"
    exit 1
fi

ADB="$SDK_DIR/platform-tools/adb"
if [ ! -x "$ADB" ]; then
    ADB="$(command -v adb)" || {
        echo
        echo "*** adb NOT FOUND ***"
        echo "Looked in $SDK_DIR/platform-tools and in PATH."
        exit 1
    }
fi

# The output is shown as it comes (the phone may hold the install on its own
# confirmation dialog) and kept for the hint below.
INSTALL_LOG="$(mktemp)"
trap 'rm -f "$INSTALL_LOG"' EXIT
"$ADB" install -r app/build/outputs/apk/debug/app-debug.apk 2>&1 | tee "$INSTALL_LOG"
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
    echo
    echo "*** INSTALL FAILED ***"
    # A debug build is signed with the debug key of the machine that built it.
    # The phone refuses an update signed by another machine's key.
    if grep -q 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$INSTALL_LOG"; then
        echo "The installed app is signed with another key (built on another machine)."
        echo "Either copy that machine's ~/.android/debug.keystore here, or remove"
        echo "the app - this deletes its settings and data on the phone:"
        echo "    \"$ADB\" uninstall $PACKAGE"
    fi
    exit 1
fi

# All-files access is needed to read neurodoc catalogs from shared storage
"$ADB" shell appops set --uid "$PACKAGE" MANAGE_EXTERNAL_STORAGE allow
"$ADB" shell am start -n "$PACKAGE/.MainActivity"

echo
echo "Installed and started on the device."
