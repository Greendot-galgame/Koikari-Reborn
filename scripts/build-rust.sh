#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$ROOT/.build-tools/bin:$PATH"
PLATFORM="${1:-macosx}"
case "$PLATFORM" in
  macosx) TARGET=aarch64-apple-darwin; export MACOSX_DEPLOYMENT_TARGET=13.0 ;;
  iphonesimulator) TARGET=aarch64-apple-ios-sim; export IPHONEOS_DEPLOYMENT_TARGET=16.0 ;;
  iphoneos) TARGET=aarch64-apple-ios; export IPHONEOS_DEPLOYMENT_TARGET=16.0 ;;
  *) echo "Unsupported platform: $PLATFORM" >&2; exit 2 ;;
esac
command -v cargo >/dev/null || { echo "Install Rust via https://rustup.rs first." >&2; exit 1; }
command -v cmake >/dev/null || { echo "CMake is required. Run: python3 -m venv '$ROOT/.build-tools' && '$ROOT/.build-tools/bin/pip' install cmake==3.31.10" >&2; exit 1; }
rustup target add "$TARGET"
export OPUS_NO_PKG=1
export KOIKARI_PLATFORM="$PLATFORM"
export KOIKARI_SDKROOT="$(xcrun --sdk "$PLATFORM" --show-sdk-path)"
export CMAKE_TOOLCHAIN_FILE="$ROOT/scripts/apple-toolchain.cmake"
cd "$ROOT"
mkdir -p "$ROOT/target/apple/$PLATFORM"
STAMP="$ROOT/target/apple/$PLATFORM/opus-toolchain.sha256"
HASH="$(printf '%s\n' "$KOIKARI_SDKROOT" "$PLATFORM" "13.0/16.0" "$(cat "$CMAKE_TOOLCHAIN_FILE")" | shasum -a 256)"
if [[ ! -f "$STAMP" ]] || [[ "$(cat "$STAMP")" != "$HASH" ]]; then
  cargo clean -p audiopus_sys --target "$TARGET"
fi
PROFILE=debug
if [[ "${CONFIGURATION:-Debug}" == Release ]]; then
  PROFILE=release
  cargo build --locked --lib --target "$TARGET" --release
else
  cargo build --locked --lib --target "$TARGET"
fi
cp "$ROOT/target/$TARGET/$PROFILE/libkoikari_reborn.a" "$ROOT/target/apple/$PLATFORM/libkoikari_reborn.a"
printf '%s\n' "$HASH" > "$STAMP"
