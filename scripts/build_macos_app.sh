#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_vision"
APP_NAME="PhotonVision"
APP_BUNDLE="/Applications/${APP_NAME}.app"

cd "$PROJECT_DIR"
cargo build --release

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$PROJECT_DIR/target/release/photon_vision" "$APP_BUNDLE/Contents/MacOS/PhotonVision"
chmod +x "$APP_BUNDLE/Contents/MacOS/PhotonVision"

cat > "$APP_BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>PhotonVision</string>
    <key>CFBundleIdentifier</key>
    <string>com.pietiniart.photonvision</string>
    <key>CFBundleName</key>
    <string>PhotonVision</string>
    <key>CFBundleDisplayName</key>
    <string>PhotonVision</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Microsoft Visio &amp; PhotonVision Diagram</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>vsdx</string>
                <string>vssx</string>
                <string>vdx</string>
                <string>vsd</string>
                <string>pvis</string>
                <string>lbrn2</string>
                <string>lbrn</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

if security find-identity -v -p codesigning | grep -q "Apple Development"; then
    echo "🔏 Allekirjoitetaan Developer-sertifikaatilla..."
    codesign --deep --force --options runtime --sign "Apple Development: Niko Pietinen (DTG24Y4L3C)" "$APP_BUNDLE" || true
else
    codesign --deep --force --sign - "$APP_BUNDLE" || true
fi

echo "✅ Paketoitu ja allekirjoitettu valmiiksi: $APP_BUNDLE"
