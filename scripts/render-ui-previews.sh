#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="$ROOT/target/ui-previews"
ARTWORK=""
if [[ "$#" != 0 ]]; then
  if [[ "$#" != 4 || "$1" != --artwork-directory || "$3" != --output ]]; then
    echo 'Usage: render-ui-previews.sh [--artwork-directory PRIVATE_IMAGE_DIR --output PRIVATE_OUTPUT_DIR]' >&2
    exit 2
  fi
  ARTWORK="$2"
  OUTPUT="$4"
  python3 - "$ROOT" "$OUTPUT" <<'PY'
import pathlib, sys
if pathlib.Path(sys.argv[2]).resolve().is_relative_to(pathlib.Path(sys.argv[1]).resolve()):
    raise SystemExit('Previews containing original artwork must stay outside the repository.')
PY
fi
mkdir -p "$OUTPUT"
# Use the actual value models; the fixture never approximates their data contracts.
python3 - "$ROOT/apple/KoikariApp.swift" "$OUTPUT/SaveSlot.swift" <<'PY'
import pathlib
import sys
source = pathlib.Path(sys.argv[1]).read_text()
collection = source[source.index('struct CollectionEntry:'):source.index('struct Dialogue:')]
start = source.index('struct PastPage:')
stop = source.index('// BEGIN MEDIA LIFECYCLE POLICY', start)
pathlib.Path(sys.argv[2]).write_text('import Foundation\n' + collection + source[start:stop])
PY
xcrun --sdk macosx swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
  -parse-as-library -D KOIKARI_UI_PREVIEW -target arm64-apple-macos13.0 \
  -sdk "$(xcrun --sdk macosx --show-sdk-path)" \
  "$OUTPUT/SaveSlot.swift" "$ROOT/apple/ReadingPolicy.swift" "$ROOT/apple/GameUI.swift" \
  "$ROOT/apple/OriginalUI.swift" "$ROOT/apple/GalleryThumbnails.swift" "$ROOT/apple/GameMenus.swift" \
  "$ROOT/apple/TitleScreen.swift" "$ROOT/apple/SettingsView.swift" "$ROOT/tests/ui_preview.swift" \
  -o "$OUTPUT/render-ui"
if [[ -n "$ARTWORK" ]]; then
  "$OUTPUT/render-ui" "$OUTPUT" "$ARTWORK"
else
  "$OUTPUT/render-ui" "$OUTPUT"
fi
printf 'Visual fixtures: %s\n' "$OUTPUT"
