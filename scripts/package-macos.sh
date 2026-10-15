#!/usr/bin/env bash
# Builds SimpleEditor.app, a zip and a DMG in dist/ for the host architecture.
#
# Signing:
#   - Default: ad-hoc signature (runs locally; Gatekeeper will warn on other
#     Macs because it is not signed with a Developer ID nor notarized).
#   - SE_CODESIGN_IDENTITY="Developer ID Application: …": signs with the
#     hardened runtime. Requires the owner's certificate in the keychain.
#   - SE_NOTARY_PROFILE=<keychain profile>: notarizes and staples with
#     `xcrun notarytool` (requires the owner's Apple Developer account).
# Nothing is uploaded or published.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(cargo metadata --no-deps --format-version 1 --locked \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)["packages"][0]["version"])')
ARCH=$(uname -m)
# Owner decision: reverse-DNS identifier (see docs/RELEASE.md).
BUNDLE_ID="${SE_BUNDLE_ID:-io.github.enshetinin.simpleeditor}"
DIST=dist
APP="$DIST/SimpleEditor.app"
NAME="SimpleEditor-$VERSION-macos-$ARCH"

cargo build --release --locked --bins

rm -rf "$APP" "$DIST/$NAME.zip" "$DIST/$NAME.dmg" "$DIST/dmg"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/simpleeditor "$APP/Contents/MacOS/SimpleEditor"
cp target/release/se "$APP/Contents/MacOS/se"
cp LICENSE THIRD_PARTY_LICENSES.md README.md "$APP/Contents/Resources/"

# Icon: iconset from the generated PNGs (scripts/make_icon.py).
ICONSET="$DIST/AppIcon.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  cp "assets/icon/icon-$s.png" "$ICONSET/icon_${s}x${s}.png"
  d=$((s * 2))
  cp "assets/icon/icon-$d.png" "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

# Minimum macOS version as recorded in the binary (not assumed).
MINOS=$(otool -l "$APP/Contents/MacOS/SimpleEditor" | awk '/LC_BUILD_VERSION/{f=1} f && /minos/{print $2; exit}')
COPYRIGHT=$(grep -m1 '^Copyright' LICENSE)

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>SimpleEditor</string>
  <key>CFBundleDisplayName</key><string>SimpleEditor</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>SimpleEditor</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>$MINOS</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>$COPYRIGHT. MIT License.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist"

if [[ -n "${SE_CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$SE_CODESIGN_IDENTITY" "$APP/Contents/MacOS/se"
  codesign --force --options runtime --timestamp --sign "$SE_CODESIGN_IDENTITY" "$APP"
  SIGNED="Developer ID ($SE_CODESIGN_IDENTITY)"
else
  codesign --force --sign - "$APP/Contents/MacOS/se"
  codesign --force --sign - "$APP"
  SIGNED="ad-hoc (not distributable without warnings)"
fi
codesign --verify --strict --verbose=2 "$APP"

ditto -c -k --keepParent "$APP" "$DIST/$NAME.zip"
mkdir -p "$DIST/dmg"
cp -R "$APP" "$DIST/dmg/"
ln -s /Applications "$DIST/dmg/Applications"
hdiutil create -volname SimpleEditor -srcfolder "$DIST/dmg" -ov -format UDZO "$DIST/$NAME.dmg" >/dev/null
rm -rf "$DIST/dmg"

if [[ -n "${SE_NOTARY_PROFILE:-}" ]]; then
  xcrun notarytool submit "$DIST/$NAME.dmg" --keychain-profile "$SE_NOTARY_PROFILE" --wait
  xcrun stapler staple "$DIST/$NAME.dmg"
  NOTARIZED=yes
else
  NOTARIZED=no
fi

(cd "$DIST" && shasum -a 256 "$NAME.zip" "$NAME.dmg" > "$NAME.sha256")
echo "Built $APP ($ARCH, macOS >= $MINOS, signature: $SIGNED, notarized: $NOTARIZED)"
ls -la "$DIST"
