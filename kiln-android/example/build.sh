#!/bin/sh
# Build the Rust library for Android and wrap it in an APK.
#
# Kept as a shell script rather than a Gradle task chain so the steps are visible:
# cross-compile, stage the .so, let Gradle package it.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
ABI=${ABI:-arm64-v8a}
TARGET=aarch64-linux-android

export ANDROID_HOME=${ANDROID_HOME:-/opt/android-sdk}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-/opt/android-ndk}
export NDK_TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"

export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android26-clang"
export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/aarch64-linux-android26-clang"
# wry's Android build script generates Kotlin sources and needs these.
export WRY_ANDROID_PACKAGE=com.kiln.example
export WRY_ANDROID_LIBRARY=kiln_example
export WRY_ANDROID_KOTLIN_FILES_OUT_DIR="$HERE/app/src/main/java/com/kiln/example"

echo "==> building Rust for $TARGET"
(cd "$HERE" && cargo build --release --target "$TARGET")

echo "==> staging the shared library"
mkdir -p "$HERE/app/src/main/jniLibs/$ABI"
cp "$HERE/target/$TARGET/release/libkiln_example.so" \
   "$HERE/app/src/main/jniLibs/$ABI/libkiln_example.so"

echo "==> packaging with Gradle"
cd "$HERE/app"
gradle assembleDebug --no-daemon -q

APK=$(find "$HERE/app/build/outputs/apk" -name '*.apk' | head -1)
echo "==> APK: $APK"
