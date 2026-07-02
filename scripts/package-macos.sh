#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(python3 - <<'PY'
from pathlib import Path
for line in Path("Cargo.toml").read_text(encoding="utf-8").splitlines():
    line = line.strip()
    if line.startswith("version"):
        print(line.split("=", 1)[1].strip().strip('"'))
        break
PY
)"
OUT_DIR="${1:-dist}"
TARGET="${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
ARCH="${TARGET%%-*}"
APP_NAME="Claude Clone"
BUNDLE_ID="${BUNDLE_ID:-com.sxhxliang.claude-clone}"
EXECUTABLE_NAME="claude_clone"
WORK_DIR="${OUT_DIR}/macos-${TARGET}"
APP_BUNDLE="${WORK_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_BUNDLE}/Contents"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
ICONSET="${WORK_DIR}/claude_clone.iconset"
ICNS="${RESOURCES_DIR}/claude_clone.icns"
ASSET="claude_clone-v${VERSION}-macos-${ARCH}.dmg"

rm -rf "$WORK_DIR"
mkdir -p "$RESOURCES_DIR" "$MACOS_DIR" "$ICONSET" "$OUT_DIR"

cargo build --release
cp "target/release/${EXECUTABLE_NAME}" "${MACOS_DIR}/${EXECUTABLE_NAME}"
chmod +x "${MACOS_DIR}/${EXECUTABLE_NAME}"

cp assets/icons/png/claude_clone-16.png "${ICONSET}/icon_16x16.png"
cp assets/icons/png/claude_clone-32.png "${ICONSET}/icon_16x16@2x.png"
cp assets/icons/png/claude_clone-32.png "${ICONSET}/icon_32x32.png"
cp assets/icons/png/claude_clone-64.png "${ICONSET}/icon_32x32@2x.png"
cp assets/icons/png/claude_clone-128.png "${ICONSET}/icon_128x128.png"
cp assets/icons/png/claude_clone-256.png "${ICONSET}/icon_128x128@2x.png"
cp assets/icons/png/claude_clone-256.png "${ICONSET}/icon_256x256.png"
cp assets/icons/png/claude_clone-512.png "${ICONSET}/icon_256x256@2x.png"
cp assets/icons/png/claude_clone-512.png "${ICONSET}/icon_512x512.png"
cp assets/icons/png/claude_clone-1024.png "${ICONSET}/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$ICNS"

cat > "${CONTENTS_DIR}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleExecutable</key>
  <string>${EXECUTABLE_NAME}</string>
  <key>CFBundleIconFile</key>
  <string>claude_clone</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

if [[ -n "${MACOS_CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_CODESIGN_IDENTITY" "$APP_BUNDLE"
else
  codesign --force --deep --sign - "$APP_BUNDLE"
fi

hdiutil create -quiet -volname "$APP_NAME" -srcfolder "$APP_BUNDLE" -ov -format UDZO "${OUT_DIR}/${ASSET}"

echo "${OUT_DIR}/${ASSET}"
