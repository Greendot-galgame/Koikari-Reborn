#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-macosx}"
case "$PLATFORM" in macosx|iphonesimulator|iphoneos) ;; *) echo 'Usage: build-apple.sh macosx|iphonesimulator|iphoneos' >&2; exit 2 ;; esac
xcodebuild -project "$ROOT/apple/KoikariReborn.xcodeproj" -scheme KoikariReborn \
  -configuration "${CONFIGURATION:-Debug}" -sdk "$PLATFORM" \
  -derivedDataPath "$ROOT/target/xcode-$PLATFORM" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO build
SUFFIX="-$PLATFORM"
if [[ "$PLATFORM" == macosx ]]; then SUFFIX=""; fi
echo "App: $ROOT/target/xcode-$PLATFORM/Build/Products/${CONFIGURATION:-Debug}$SUFFIX/KoikariReborn.app"
