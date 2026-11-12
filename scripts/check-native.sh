#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Native checks require macOS and full Xcode.' >&2
  exit 2
fi
WORK="$(mktemp -d "${TMPDIR:-/tmp}/koikari-native.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

xcrun swift-format lint --strict apple/*.swift tests/*.swift
for SDK in macosx iphonesimulator iphoneos; do
  case "$SDK" in
    macosx) TARGET=arm64-apple-macos13.0 ;;
    iphonesimulator) TARGET=arm64-apple-ios16.0-simulator ;;
    iphoneos) TARGET=arm64-apple-ios16.0 ;;
  esac
  SDKROOT="$(xcrun --sdk "$SDK" --show-sdk-path)"
  xcrun --sdk "$SDK" swiftc -typecheck -swift-version 6 \
    -strict-concurrency=complete -warnings-as-errors \
    -target "$TARGET" -sdk "$SDKROOT" -import-objc-header apple/Koikari.h apple/*.swift
  xcrun --sdk "$SDK" metal -c src/scene.metal -o "$WORK/$SDK.air"
done

python3 - "$ROOT/apple/KoikariApp.swift" "$WORK" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text()
output = pathlib.Path(sys.argv[2])

def region(begin, end):
    if source.count(begin) != 1 or source.count(end) != 1:
        raise SystemExit(f"Expected unique source boundary: {begin} / {end}")
    start = source.index(begin)
    stop = source.index(end, start)
    return source[start:stop]

native = region("struct StoryChoice:", "struct StoryProgress:")
native += region("// BEGIN AUDIO FAILURE POLICY", "// END AUDIO FAILURE POLICY")
(output / "NativePolicies.swift").write_text("import Foundation\n" + native)
media = region("// BEGIN MEDIA LIFECYCLE POLICY", "// END MEDIA LIFECYCLE POLICY")
(output / "MediaPolicy.swift").write_text(media)
PY

compile_run() {
  local NAME="$1"
  shift
  xcrun swiftc -swift-version 6 -strict-concurrency=complete \
    -warnings-as-errors -parse-as-library "$@" -o "$WORK/$NAME"
  "$WORK/$NAME"
}
compile_run native "$WORK/NativePolicies.swift" tests/native_integration.swift
compile_run media "$WORK/MediaPolicy.swift" tests/media_lifecycle.swift
compile_run import apple/AssetImport.swift tests/asset_import.swift
compile_run location apple/AssetLocation.swift tests/asset_location.swift
echo 'Native checks passed; no game resources or UI were used.'
