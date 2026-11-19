#!/bin/sh
# Makes Pina Live.app from a built `pina-live` program: its Info.plist, icon and entitlements,
# signed with the hardened runtime (by PINA_SIGN_ID's Developer ID if it names one, else ad hoc).
#
#   scripts/mac-app.sh <program> <path/to/Pina Live.app>
set -eu
cd "$(dirname "$0")/.."
BIN="$1"
APP="$2"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
YEAR=$(date +%Y)

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/pina-live"
cp crates/app/icon/live.icns "$APP/Contents/Resources/live.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Pina Live</string>
  <key>CFBundleDisplayName</key><string>Pina Live</string>
  <key>CFBundleIdentifier</key><string>studio.pina.live</string>
  <key>CFBundleExecutable</key><string>pina-live</string>
  <key>CFBundleIconFile</key><string>live</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>NSHumanReadableCopyright</key><string>© $YEAR Pina Studio</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSMinimumSystemVersionByArchitecture</key><dict><key>arm64</key><string>13.4</string><key>x86_64</key><string>13.0</string></dict>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
codesign --force --options runtime --entitlements scripts/live.entitlements \
  ${PINA_SIGN_ID:+--timestamp} --sign "${PINA_SIGN_ID:--}" "$APP"
