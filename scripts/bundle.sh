#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

IDENTITY="Taywind Dev"
LOG="$HOME/Library/Logs/TaywindRecord.log"
STAGE="$PWD/dist/dmg"
DMG="$PWD/dist/TaywindRecord.dmg"

if ! security find-identity -p codesigning | grep -q "\"$IDENTITY\""; then
  echo "Code signing identity \"$IDENTITY\" not found; see docs/development.md (Signing identity)" >&2
  exit 1
fi

if [ "${1:-}" = "--dmg" ]; then
  cargo build --release
  BIN=target/release/taywind_record
  APP="$STAGE/TaywindRecord.app"
  rm -rf "$STAGE"
else
  cargo build
  BIN=target/debug/taywind_record
  APP="$PWD/dist/TaywindRecord.app"
  pkill -x taywind_record || true
  for _ in $(seq 50); do pgrep -x taywind_record >/dev/null || break; sleep 0.1; done
  pkill -9 -x taywind_record || true
fi

mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/taywind_record"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.taywind.record</string>
  <key>CFBundleName</key><string>Taywind Record</string>
  <key>CFBundleExecutable</key><string>taywind_record</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSCameraUsageDescription</key><string>Show and record your face in the camera bubble.</string>
  <key>NSMicrophoneUsageDescription</key><string>Record your voice-over.</string>
  <key>NSScreenCaptureUsageDescription</key><string>Record your screen for demo videos.</string>
</dict>
</plist>
EOF

codesign --force --sign "$IDENTITY" "$APP"

if [ "${1:-}" = "--dmg" ]; then
  ln -s /Applications "$STAGE/Applications"
  hdiutil create -volname "Taywind Record" -srcfolder "$STAGE" -format UDZO -ov "$DMG"
  rm -rf "$STAGE"
  exit 0
fi

: > "$LOG"
open --stdout "$LOG" --stderr "$LOG" "$APP"
