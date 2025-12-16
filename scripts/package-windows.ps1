# Windows packaging script for Smart Relay
# Creates a release zip with both CLI and GUI binaries

$ErrorActionPreference = "Stop"

Write-Host "Building Smart Relay for Windows..." -ForegroundColor Cyan

# Build release binaries
cargo build --release --bin smart-relay
cargo build --release --bin smart-relay-gui

if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed!" -ForegroundColor Red
    exit 1
}

$VERSION = (Get-Content Cargo.toml | Select-String '^version = "([^"]+)"').Matches.Groups[1].Value
$RELEASE_DIR = "target/release"
$PACKAGE_DIR = "smart-relay-windows-x86_64-v$VERSION"
$ZIP_FILE = "$PACKAGE_DIR.zip"

# Create package directory
if (Test-Path $PACKAGE_DIR) {
    Remove-Item -Recurse -Force $PACKAGE_DIR
}
New-Item -ItemType Directory -Path $PACKAGE_DIR | Out-Null

# Copy binaries
Copy-Item "$RELEASE_DIR/smart-relay.exe" "$PACKAGE_DIR/"
Copy-Item "$RELEASE_DIR/smart-relay-gui.exe" "$PACKAGE_DIR/"

# Create README
@"
Smart Relay v$VERSION - Windows x86_64

Usage:
  smart-relay.exe serve --listen 127.0.0.1:8080    # Start control plane
  smart-relay-gui.exe                              # Launch GUI

For more information, see: https://github.com/your-repo/smart-relay
"@ | Out-File -FilePath "$PACKAGE_DIR/README.txt" -Encoding UTF8

# Create zip
if (Test-Path $ZIP_FILE) {
    Remove-Item $ZIP_FILE
}
Compress-Archive -Path "$PACKAGE_DIR/*" -DestinationPath $ZIP_FILE

Write-Host "Package created: $ZIP_FILE" -ForegroundColor Green

