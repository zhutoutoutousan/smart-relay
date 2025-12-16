#!/bin/bash
# macOS packaging script for Smart Relay
# Creates .app bundle and .dmg

set -e

echo "Building Smart Relay for macOS..."

# Build release binaries
cargo build --release --bin smart-relay
cargo build --release --bin smart-relay-gui

VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
RELEASE_DIR="target/release"
APP_NAME="Smart Relay.app"
APP_DIR="$APP_NAME/Contents/MacOS"
DMG_NAME="smart-relay-macos-x86_64-v${VERSION}.dmg"

# Create .app bundle structure
rm -rf "$APP_NAME"
mkdir -p "$APP_DIR"

# Copy binaries
cp "$RELEASE_DIR/smart-relay" "$APP_DIR/"
cp "$RELEASE_DIR/smart-relay-gui" "$APP_DIR/"
chmod +x "$APP_DIR"/*

# Create Info.plist
cat > "$APP_NAME/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>smart-relay-gui</string>
    <key>CFBundleIdentifier</key>
    <string>com.smartrelay.app</string>
    <key>CFBundleName</key>
    <string>Smart Relay</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
</dict>
</plist>
EOF

# Create DMG (requires hdiutil)
if command -v hdiutil &> /dev/null; then
    rm -f "$DMG_NAME"
    hdiutil create -volname "Smart Relay" -srcfolder "$APP_NAME" -ov -format UDZO "$DMG_NAME"
    echo "DMG created: $DMG_NAME"
else
    echo "hdiutil not found, skipping DMG creation"
    echo "App bundle created: $APP_NAME"
fi

