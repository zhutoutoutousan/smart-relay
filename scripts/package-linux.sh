#!/bin/bash
# Linux packaging script for Smart Relay
# Creates AppImage and .deb packages

set -e

echo "Building Smart Relay for Linux..."

# Build release binaries
cargo build --release --bin smart-relay
cargo build --release --bin smart-relay-gui

VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
RELEASE_DIR="target/release"
PACKAGE_DIR="smart-relay-linux-x86_64-v${VERSION}"

# Create package directory
rm -rf "$PACKAGE_DIR"
mkdir -p "$PACKAGE_DIR"

# Copy binaries
cp "$RELEASE_DIR/smart-relay" "$PACKAGE_DIR/"
cp "$RELEASE_DIR/smart-relay-gui" "$PACKAGE_DIR/"
chmod +x "$PACKAGE_DIR"/*

# Create README
cat > "$PACKAGE_DIR/README.txt" <<EOF
Smart Relay v${VERSION} - Linux x86_64

Usage:
  ./smart-relay serve --listen 127.0.0.1:8080    # Start control plane
  ./smart-relay-gui                               # Launch GUI

For more information, see: https://github.com/your-repo/smart-relay
EOF

# Create tarball
tar -czf "${PACKAGE_DIR}.tar.gz" "$PACKAGE_DIR"

echo "Package created: ${PACKAGE_DIR}.tar.gz"

# Note: .deb packaging would require additional tools (dpkg-deb, etc.)
# This is a basic tarball for now

