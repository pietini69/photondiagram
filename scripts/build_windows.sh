#!/usr/bin/env bash
set -euo pipefail

# Build PhotonDiagram for Windows (x86_64-pc-windows-msvc) using cargo-xwin

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building PhotonDiagram v${VERSION} for Windows (x86_64-pc-windows-msvc)"
echo "=========================================="

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
cargo xwin build --release --bin photondiagram --target x86_64-pc-windows-msvc

STAGE_DIR="$DIST_DIR/PhotonDiagram-${VERSION}-windows-x64"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

cp "$PROJECT_DIR/target/x86_64-pc-windows-msvc/release/photondiagram.exe" "$STAGE_DIR/PhotonDiagram.exe"

cat << EOF > "$STAGE_DIR/README.txt"
PhotonDiagram v${VERSION} (Windows x64)
=======================================
Modern, high-performance technical diagramming, electrical schematics,
floor plans, and CAD drawing tool.

Usage:
  1. Extract this zip archive into any folder (e.g. C:\PhotonDiagram).
  2. Double-click PhotonDiagram.exe to start.

First Launch (Windows Defender SmartScreen):
  If the blue "Windows protected your PC" screen appears:
  - Click "More info"
  - Click "Run anyway"
  (This only needs to be clicked once; Windows will remember your choice).
EOF

ZIP_PATH="$DIST_DIR/PhotonDiagram-${VERSION}-windows-x64.zip"
rm -f "$ZIP_PATH"
(cd "$STAGE_DIR" && zip -r "$ZIP_PATH" PhotonDiagram.exe README.txt)
rm -rf "$STAGE_DIR"

echo "==> Windows package created: $ZIP_PATH"
