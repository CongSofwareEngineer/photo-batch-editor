#!/usr/bin/env bash
# PRODUCTION BUILD FOR macOS (MacBook) - run on a Mac (PyInstaller cannot build a Mac app on Windows).
#   tests -> build number +1 -> dist/PhotoBatchEditor.app -> dist/PhotoBatchEditor-<version>-<arch>.dmg
#   ./build_mac.sh               full build
#   ./build_mac.sh --skip-tests  skip pytest
# The app is built for the CPU of this Mac: Apple Silicon (arm64, M1-M4) or Intel (x86_64).
# Needs Python 3.11 or 3.12 (brew install python@3.12, or python.org). For day-to-day work use ./dev.sh.
set -euo pipefail
cd "$(dirname "$0")"

PY_BIN="${PYTHON:-}"
if [ -z "$PY_BIN" ]; then
    for c in python3.12 python3.11 python3; do
        if command -v "$c" >/dev/null 2>&1; then PY_BIN="$c"; break; fi
    done
fi
[ -n "$PY_BIN" ] || { echo "Python 3.11/3.12 not found: brew install python@3.12"; exit 1; }

if [ ! -x venv/bin/python ]; then
    echo "=== Creating venv/ with $PY_BIN ==="
    "$PY_BIN" -m venv venv
    venv/bin/python -m pip install --upgrade pip
    venv/bin/pip install -r requirements-dev.txt
fi
PY=venv/bin/python

if [ "${1:-}" != "--skip-tests" ]; then
    echo "=== Lint + type check ==="
    bash ./lint.sh --check
    echo "=== Tests ==="
    QT_QPA_PLATFORM=offscreen "$PY" -m pytest -q
fi

echo "=== FFmpeg (Video editor) ==="
"$PY" tools/fetch_ffmpeg.py

echo "=== Icon ==="
[ -f assets/app.icns ] || "$PY" tools/make_icns.py

echo "=== Version ==="
VERSION="$("$PY" tools/bump_build.py)"
echo "Version $VERSION"

echo "=== PyInstaller ==="
"$PY" -m PyInstaller --noconfirm PhotoBatchEditor.spec
APP="dist/PhotoBatchEditor.app"
# Ad-hoc signature so Apple Silicon runs it (no Apple Developer ID needed)
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || echo "codesign skipped"
echo "Built: $APP"

echo "=== DMG ==="
ARCH="$(uname -m)"
DMG="dist/PhotoBatchEditor-$VERSION-$ARCH.dmg"
STAGE="build/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Photo Batch Editor" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
echo "Installer: $DMG  (open it and drag Photo Batch Editor into Applications)"
echo "First start of an unsigned app: right-click the app > Open, or run"
echo "  xattr -dr com.apple.quarantine /Applications/PhotoBatchEditor.app"
