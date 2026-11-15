#!/bin/sh
# Headless end-to-end check for the Android build.
#
# Installs the APK on a connected device or emulator, launches it, and asserts that
# the page loaded *and* that a command round-tripped: the Rust side logs when
# `greet` runs, which is proof the bridge works rather than that a window exists.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ADB=${ADB:-$ANDROID_HOME/platform-tools/adb}
[ -x "$ADB" ] || ADB=$(command -v adb)

APK="$HERE/app/build/outputs/apk/debug/kiln-example-debug.apk"
[ -f "$APK" ] || { echo "no APK: run ./build.sh first" >&2; exit 1; }

echo "==> installing"
"$ADB" install -r "$APK" >/dev/null

echo "==> launching"
"$ADB" shell am force-stop com.kiln.example
"$ADB" logcat -c
"$ADB" shell am start -n com.kiln.example/.MainActivity >/dev/null

# Give the WebView time to start and the page's auto-invoke to fire.
sleep 16

LOG=$("$ADB" logcat -d -s Kiln:* 2>/dev/null || true)

fail=0
if ! echo "$LOG" | grep -q "onWebViewCreate"; then
  echo "FAIL webview was never attached" >&2
  fail=1
else
  echo "ok   webview attached"
fi

if echo "$LOG" | grep -q "greet invoked"; then
  echo "ok   command round-tripped into Rust"
else
  echo "FAIL no command reached Rust" >&2
  fail=1
fi

if "$ADB" logcat -d 2>/dev/null | grep -q "FATAL EXCEPTION"; then
  echo "FAIL app crashed" >&2
  "$ADB" logcat -d 2>/dev/null | grep -A6 "FATAL EXCEPTION" | head -10 >&2
  fail=1
else
  echo "ok   no crash"
fi

exit "$fail"
