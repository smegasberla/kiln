#!/bin/sh
# Build the Rust library for Android and wrap it in an APK.
#
# Kept as a shell script rather than a Gradle task chain so the steps stay visible:
# cross-compile, stage the .so, let Gradle package it.
#
# ABI selects the target: arm64-v8a (phones, the shipping target) or x86_64
# (emulators on x86 hosts, which is what CI runs).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ABI=${ABI:-arm64-v8a}

case "$ABI" in
  arm64-v8a)   TARGET=aarch64-linux-android; CC_PREFIX=aarch64-linux-android; API=26 ;;
  x86_64)      TARGET=x86_64-linux-android;  CC_PREFIX=x86_64-linux-android;  API=26 ;;
  armeabi-v7a) TARGET=armv7-linux-androideabi; CC_PREFIX=armv7a-linux-androideabi; API=26 ;;
  *) echo "unsupported ABI: $ABI (use arm64-v8a, x86_64, or armeabi-v7a)" >&2; exit 1 ;;
esac

export ANDROID_HOME=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-/opt/android-ndk}
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
CLANG="$TOOLCHAIN/${CC_PREFIX}${API}-clang"

[ -x "$CLANG" ] || { echo "no NDK compiler at $CLANG" >&2; exit 1; }

export CC_${TARGET//-/_}="$CLANG"
export AR_${TARGET//-/_}="$TOOLCHAIN/llvm-ar"
export CARGO_TARGET_$(echo "$TARGET" | tr 'a-z-' 'A-Z_')_LINKER="$CLANG"

# wry's Android build script generates the Kotlin classes the app compiles against.
export WRY_ANDROID_PACKAGE=com.kiln.example
export WRY_ANDROID_LIBRARY=kiln_example
export WRY_ANDROID_KOTLIN_FILES_OUT_DIR="$HERE/app/src/main/java/com/kiln/example"

echo "==> building Rust for $TARGET ($ABI)"
(cd "$HERE" && cargo build --release --target "$TARGET")

echo "==> staging the shared library"
JNI="$HERE/app/src/main/jniLibs/$ABI"
mkdir -p "$JNI"
cp "$HERE/target/$TARGET/release/libkiln_example.so" "$JNI/libkiln_example.so"

echo "==> packaging with Gradle"
(cd "$HERE/app" && gradle assembleDebug --no-daemon -q)

APK=$(find "$HERE/app/build/outputs/apk" -name '*.apk' | head -1)
echo "==> APK: $APK"
