#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DEFAULT_SIGNING_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | grep "Apple Development" | head -n 1 | cut -d '"' -f 2 || true)
SIGNING_IDENTITY="${SIGNING_IDENTITY:-${DEFAULT_SIGNING_IDENTITY:-}}"
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

MODE="${1:-universal}"
export PATH="$HOME/.cargo/bin:$PATH"

package_target() {
    local target_name="$1"    # "arm64", "intel-x86_64", or "universal"
    local bin_source="$2"
    local vol_name="PhotonDiagram ${VERSION}"
    local app_dir="$DIST_DIR/PhotonDiagram_${target_name}.app"
    local dmg_root="$DIST_DIR/dmg_${target_name}_root"
    local dmg_path="$DIST_DIR/PhotonDiagram-${VERSION}-${target_name}.dmg"
    local zip_path="$DIST_DIR/PhotonDiagram-${VERSION}-macos-${target_name}.zip"

    echo "==> Constructing ${target_name} .app bundle..."
    rm -rf "$app_dir" "$dmg_root"
    mkdir -p "$app_dir/Contents/MacOS"
    mkdir -p "$app_dir/Contents/Resources"

    cp "$bin_source" "$app_dir/Contents/MacOS/photondiagram"
    chmod +x "$app_dir/Contents/MacOS/photondiagram"

    cat > "$app_dir/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>photondiagram</string>
    <key>CFBundleIdentifier</key>
    <string>com.pietiniart.photondiagram</string>
    <key>CFBundleName</key>
    <string>PhotonDiagram</string>
    <key>CFBundleDisplayName</key>
    <string>PhotonDiagram</string>
    <key>CFBundleIconFile</key>
    <string>PhotonDiagram.icns</string>
    <key>CFBundleIconName</key>
    <string>PhotonDiagram</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Technical Diagram Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>pdiag</string>
                <string>pvis</string>
                <string>vsdx</string>
                <string>vssx</string>
                <string>vdx</string>
                <string>vsd</string>
                <string>lbrn2</string>
                <string>lbrn</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

    if [ -f "$PROJECT_DIR/assets/PhotonDiagram.icns" ]; then
        cp "$PROJECT_DIR/assets/PhotonDiagram.icns" "$app_dir/Contents/Resources/PhotonDiagram.icns"
    fi

    echo "==> Code signing ${target_name} bundle..."
    if [ -n "$SIGNING_IDENTITY" ]; then
        codesign --force --deep --sign "$SIGNING_IDENTITY" "$app_dir"
        codesign --verify --deep --strict "$app_dir"
    else
        echo "    (No Apple Development signing identity found; signing ad-hoc)"
        codesign --force --deep --sign - "$app_dir"
    fi

    echo "==> Packaging DMG: $dmg_path..."
    mkdir -p "$dmg_root"
    cp -R "$app_dir" "$dmg_root/PhotonDiagram.app"
    ln -s /Applications "$dmg_root/Applications"
    rm -f "$dmg_path"
    hdiutil create -volname "$vol_name" -srcfolder "$dmg_root" -ov -format UDZO "$dmg_path"

    echo "==> Packaging ZIP: $zip_path..."
    rm -f "$zip_path"
    (cd "$DIST_DIR" && zip -r "$zip_path" "PhotonDiagram_${target_name}.app")

    if [[ "$target_name" == "universal" || "$target_name" == "arm64" ]]; then
        echo "==> Installing latest build into /Applications/PhotonDiagram.app..."
        rm -rf "/Applications/PhotonDiagram.app"
        cp -R "$app_dir" "/Applications/PhotonDiagram.app"
    fi

    rm -rf "$dmg_root" "$app_dir"
    echo "==> Done: $dmg_path"
}

build_arm64() {
    echo "==> Building arm64 release binary..."
    cargo build --release --bin photondiagram --target aarch64-apple-darwin
}

build_intel() {
    echo "==> Building x86_64 (Intel) release binary..."
    cargo build --release --bin photondiagram --target x86_64-apple-darwin
}

case "$MODE" in
    arm64)
        build_arm64
        package_target "arm64" "$PROJECT_DIR/target/aarch64-apple-darwin/release/photondiagram"
        ;;
    intel)
        build_intel
        package_target "intel-x86_64" "$PROJECT_DIR/target/x86_64-apple-darwin/release/photondiagram"
        ;;
    universal)
        build_arm64
        build_intel
        mkdir -p "$DIST_DIR"
        UNIV_BIN="$DIST_DIR/photondiagram_universal_tmp"
        echo "==> Creating Universal 2 binary with lipo..."
        lipo -create -output "$UNIV_BIN" \
            "$PROJECT_DIR/target/aarch64-apple-darwin/release/photondiagram" \
            "$PROJECT_DIR/target/x86_64-apple-darwin/release/photondiagram"
        package_target "universal" "$UNIV_BIN"
        rm -f "$UNIV_BIN"
        ;;
    *)
        echo "Usage: $0 [universal|arm64|intel]"
        exit 1
        ;;
esac

echo "=========================================="
echo " macOS Packaging Complete!"
echo "=========================================="
