#!/bin/bash
# Build Rust version for macOS: tang version -> .app (version trong Info.plist) -> ky so -> .dmg
#
# Ky so (tuy chon, can tai khoan Apple Developer $99/nam):
#   export PBE_MAC_SIGN_IDENTITY="Developer ID Application: Ten Ban (TEAMID)"
#   export PBE_NOTARY_PROFILE="pbe-notary"   # tao 1 lan: xcrun notarytool store-credentials pbe-notary
# Khong dat -> ky ad-hoc (may khac mo lan dau: chuot phai > Open, hoac xattr -dr com.apple.quarantine).
set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(pwd)"
APP_NAME="Photo Batch Editor"
APP="$ROOT/rust/target/release/bundle/osx/$APP_NAME.app"
RES="$APP/Contents/Resources"
OUT="$ROOT/installer_output"

echo "Building Photo Batch Editor (Rust) for macOS..."

if [ ! -x ffmpeg/ffmpeg ]; then
  echo "[LOI] Thieu ffmpeg/ffmpeg (ban macOS) - can cho Video editor."
  exit 1
fi
command -v cargo-bundle >/dev/null || { echo "[LOI] Chua cai cargo-bundle: cargo install cargo-bundle"; exit 1; }

# 1. Moi lan build = 1 version moi (tang "build" trong version.json)
VERSION=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' version.json)
BUILD=$(sed -n 's/.*"build": *\([0-9][0-9]*\).*/\1/p' version.json)
BUILD=$((BUILD + 1))
printf '{\n  "version": "%s",\n  "build": %s\n}\n' "$VERSION" "$BUILD" > version.json
FULL="$VERSION.$BUILD"
echo "Version: $FULL"

# 2. Build + dong goi .app
cd rust
cargo build -p pbe-gui --release
rm -rf "$APP"
cargo bundle -p pbe-gui --release
cd "$ROOT"

# 3. Tai nguyen: app tim version.json, models/, presets_builtin/, ffmpeg/ trong Contents/Resources
mkdir -p "$RES/ffmpeg"
cp version.json "$RES/"
cp -R models presets_builtin "$RES/"
cp ffmpeg/ffmpeg "$RES/ffmpeg/"
[ -f README.md ] && cp README.md "$RES/README.txt"

# Version trong Info.plist (Finder > Get Info, "About")
PLIST="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD" "$PLIST"

# 4. Ky so: tung file thuc thi ben trong truoc (ffmpeg), roi toi .app
xattr -cr "$APP"
if [ -n "${PBE_MAC_SIGN_IDENTITY:-}" ]; then
  SIGN=(codesign --force --timestamp --options runtime --sign "$PBE_MAC_SIGN_IDENTITY")
else
  echo "Chua dat PBE_MAC_SIGN_IDENTITY -> ky ad-hoc."
  SIGN=(codesign --force --sign -)
fi
"${SIGN[@]}" "$RES/ffmpeg/ffmpeg"
"${SIGN[@]}" "$APP"
codesign --verify --strict --verbose=2 "$APP"

# 5. File cai dat .dmg (keo app vao Applications)
mkdir -p "$OUT"
DMG="$OUT/PhotoBatchEditor-$FULL.dmg"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

if [ -n "${PBE_MAC_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$PBE_MAC_SIGN_IDENTITY" "$DMG"
  if [ -n "${PBE_NOTARY_PROFILE:-}" ]; then
    # Notarize: Apple quet file; xong thi Gatekeeper cho mo tren moi may Mac khong hoi.
    xcrun notarytool submit "$DMG" --keychain-profile "$PBE_NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
    xcrun stapler staple "$APP"
    spctl --assess --type open --context context:primary-signature --verbose "$DMG" || true
  else
    echo "Chua dat PBE_NOTARY_PROFILE -> chua notarize (Gatekeeper van hoi tren may khac)."
  fi
fi

echo
echo "Build complete (version $FULL):"
echo "  $APP"
echo "  $DMG   <-- file cai dat de phat hanh"
