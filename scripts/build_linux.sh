#!/usr/bin/env bash
set -euo pipefail

# Build PhotonDiagram for Linux (x86_64-unknown-linux-gnu) using cargo-zigbuild

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building PhotonDiagram v${VERSION} for Linux (x86_64-unknown-linux-gnu)"
echo "=========================================="

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

cargo zigbuild --release --bin photondiagram --target x86_64-unknown-linux-gnu

STAGE_DIR="$DIST_DIR/PhotonDiagram-${VERSION}-linux-x86_64"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

cp "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/photondiagram" "$STAGE_DIR/photondiagram"
chmod +x "$STAGE_DIR/photondiagram"
cp "$PROJECT_DIR/assets/icon_512.png" "$STAGE_DIR/photondiagram.png"

cat << 'EOF' > "$STAGE_DIR/PhotonDiagram.desktop"
[Desktop Entry]
Name=PhotonDiagram
Comment=Modern Technical Diagramming, Electrical Schematics & CAD Software
Exec=photondiagram
Icon=photondiagram
Terminal=false
Type=Application
Categories=Graphics;Engineering;Development;
EOF

cat << EOF > "$STAGE_DIR/README.txt"
PhotonDiagram v${VERSION} (Linux x86_64)
========================================
Modern, high-performance technical diagramming, electrical schematics,
floor plans, and CAD drawing tool.

Running PhotonDiagram:
  1. Ensure execution permissions:
     chmod +x photondiagram
  2. Launch directly from terminal or file manager:
     ./photondiagram

File Dialogs on Linux:
  Ensure an XDG Desktop Portal backend is installed for native open/save dialogs:
     sudo apt install xdg-desktop-portal-gtk    # Ubuntu / Debian
     sudo pacman -S xdg-desktop-portal-gtk     # Arch Linux
EOF

TAR_PATH="$DIST_DIR/PhotonDiagram-${VERSION}-linux-x86_64.tar.gz"
rm -f "$TAR_PATH"
(cd "$DIST_DIR" && tar -czvf "$TAR_PATH" "PhotonDiagram-${VERSION}-linux-x86_64")
rm -rf "$STAGE_DIR"

echo "==> Linux package created: $TAR_PATH"
