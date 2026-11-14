#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Native checks require macOS and full Xcode.' >&2
  exit 2
fi
CHECK_METAL=1
case "${1:-}" in
  '') ;;
  --swift-only) CHECK_METAL=0 ;;
  *) echo 'Usage: scripts/check-native.sh [--swift-only]' >&2; exit 2 ;;
esac
WORK="$(mktemp -d "${TMPDIR:-/tmp}/koikari-native.XXXXXX")"
# Verify the exact owned temporary target before installing cleanup.
python3 - "$WORK" "${TMPDIR:-/tmp}" <<'CHECK_TEMP'
import pathlib, sys
work, root = map(lambda p: pathlib.Path(p).resolve(), sys.argv[1:])
if work.parent != root or not work.name.startswith('koikari-native.') or not work.is_dir():
    raise SystemExit('Unexpected native-check temporary directory')
CHECK_TEMP
trap 'rm -rf -- "${WORK:?}"' EXIT

python3 - "$ROOT/apple/KoikariReborn.xcodeproj/project.pbxproj" "$ROOT/apple" <<'CHECK_PROJECT'
import json, pathlib, re, subprocess, sys
project = json.loads(subprocess.check_output(['plutil', '-convert', 'json', '-o', '-', sys.argv[1]]))
objects = project['objects']
def verify(value):
    if isinstance(value, dict):
        for child in value.values(): verify(child)
    elif isinstance(value, list):
        for child in value: verify(child)
    elif isinstance(value, str) and re.fullmatch(r'[A-Fa-f0-9]{24}', value):
        if value not in objects: raise SystemExit(f'Unresolved Xcode object: {value}')
verify(project)
target = next(o for o in objects.values() if o.get('isa') == 'PBXNativeTarget' and o.get('name') == 'KoikariReborn')
registered = set()
for phase_id in target['buildPhases']:
    phase = objects[phase_id]
    if phase['isa'] == 'PBXSourcesBuildPhase':
        for build_id in phase['files']:
            registered.add(objects[objects[build_id]['fileRef']]['path'])
expected = {p.name for p in pathlib.Path(sys.argv[2]).glob('*.swift')}
if registered != expected:
    raise SystemExit(f'Xcode Swift sources differ: missing={expected-registered}, extra={registered-expected}')
print(f'Native project: {len(expected)} Swift sources registered; object references valid')
CHECK_PROJECT

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
  if [[ "$CHECK_METAL" == 1 ]]; then
    xcrun --sdk "$SDK" metal -c src/scene.metal -o "$WORK/$SDK.air"
  fi
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
(output / "SaveSlot.swift").write_text("import Foundation\n" + region("struct SaveSlot:", "// BEGIN MEDIA LIFECYCLE POLICY"))
# Exercise the actual menu routing implementation, changing only visibility so a
# no-resource fake Game can record gates/destinations; not a parallel reimplementation.
menu = region("  private var hasPresentedModal:", "  private func document(")
menu = menu.replace("  private ", "  ")
(output / "MenuRouting.swift").write_text("@MainActor extension GameplayMenuHarness {\n" + menu + "}\n")
if "onDismiss: finishGameplayMenu" not in source or ".onChange(of: hasPresentedModal)" not in source:
    raise SystemExit("Menu dismissal/gating callbacks must stay wired to the tested routing")
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
compile_run reading apple/ReadingPolicy.swift tests/reading_ui.swift
compile_run game-menu apple/ReadingPolicy.swift "$WORK/MenuRouting.swift" tests/game_menu.swift
compile_run artwork apple/OriginalUI.swift tests/ui_artwork.swift
compile_run thumbnails apple/GalleryThumbnails.swift tests/gallery_thumbnails.swift
compile_run save-thumbnails apple/GalleryThumbnails.swift apple/SaveThumbnails.swift tests/save_thumbnails.swift
compile_run save-slots "$WORK/SaveSlot.swift" tests/save_slots.swift
if [[ "$CHECK_METAL" == 1 ]]; then
  echo 'Native checks passed; no game resources or UI were used.'
else
  echo 'Swift checks and native policies passed; Metal shader compilation explicitly skipped.'
  echo 'No game resources or UI were used; this is not the full native check.'
fi
