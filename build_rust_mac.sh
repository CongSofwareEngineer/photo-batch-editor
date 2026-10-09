#!/bin/bash
# Build Rust version for macOS
set -e

echo "Building Photo Batch Editor (Rust) for macOS..."

# Build release
cd "$(dirname "$0")/rust"
cargo build -p pbe-gui --release

# Bundle with cargo-bundle
cargo bundle --release

# Copy resources if needed
RESOURCES_DIR="target/release/bundle/osx/Photo Batch Editor.app/Contents/Resources"
mkdir -p "$RESOURCES_DIR"

# Copy assets
if [ -d "../assets" ]; then
  cp -R ../assets/* "$RESOURCES_DIR/" 2>/dev/null || true
fi

# Copy models
if [ -d "../models" ]; then
  cp -R ../models "$RESOURCES_DIR/" 2>/dev/null || true
fi

# Copy ffmpeg
if [ -d "../ffmpeg" ]; then
  mkdir -p "$RESOURCES_DIR/ffmpeg"
  cp ../ffmpeg/ffmpeg "$RESOURCES_DIR/ffmpeg/" 2>/dev/null || true
  cp ../ffmpeg/README.txt "$RESOURCES_DIR/" 2>/dev/null || true
fi

# Copy README
if [ -f "../README.md" ]; then
  cp ../README.md "$RESOURCES_DIR/README.txt" 2>/dev/null || true
fi

echo "Build complete:"
echo "  $(pwd)/target/release/bundle/osx/Photo Batch Editor.app"
echo "  $(pwd)/target/release/photo-batch-editor"
