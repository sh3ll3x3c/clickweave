#!/bin/bash
set -euo pipefail

# Build signed + notarized macOS DMG via Tauri
#
# Environment variables (required):
#   SIGN_IDENTITY         - Code signing identity (e.g., "Developer ID Application: ...")
#   NOTARY_API_KEY        - Path to App Store Connect API key (.p8 file)
#   NOTARY_API_KEY_ID     - App Store Connect API Key ID
#   NOTARY_API_ISSUER     - App Store Connect API Issuer ID

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Validate required environment variables
: "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"
: "${NOTARY_API_KEY:?NOTARY_API_KEY is required}"
: "${NOTARY_API_KEY_ID:?NOTARY_API_KEY_ID is required}"
: "${NOTARY_API_ISSUER:?NOTARY_API_ISSUER is required}"

# Validate key file exists
if [[ ! -f "$NOTARY_API_KEY" ]]; then
    echo "Error: API key not found at $NOTARY_API_KEY"
    exit 1
fi

echo "Building signed + notarized DMG..."

# Tauri v2 env vars for code signing and notarization
export APPLE_SIGNING_IDENTITY="$SIGN_IDENTITY"
export APPLE_API_KEY_PATH="$NOTARY_API_KEY"
export APPLE_API_KEY="$NOTARY_API_KEY_ID"
export APPLE_API_ISSUER="$NOTARY_API_ISSUER"

cd "$PROJECT_ROOT"
cargo tauri build

# Find the built DMG
DMG_DIR="$PROJECT_ROOT/target/release/bundle/dmg"
DMG=$(find "$DMG_DIR" -name '*.dmg' -type f | head -1)

if [[ -z "$DMG" ]]; then
    echo "Error: No DMG found in $DMG_DIR"
    exit 1
fi

# Notarize the DMG (Tauri only notarizes the .app inside)
echo "Notarizing DMG..."
xcrun notarytool submit "$DMG" \
    --key "$NOTARY_API_KEY" \
    --key-id "$NOTARY_API_KEY_ID" \
    --issuer "$NOTARY_API_ISSUER" \
    --wait

echo "Stapling notarization ticket..."
xcrun stapler staple "$DMG"

echo "Verifying stapled DMG..."
xcrun stapler validate "$DMG"

echo ""
echo "Generating checksum..."
shasum -a 256 "$DMG" | tee "${DMG}.sha256"

echo ""
echo "Success! Notarized DMG: $DMG"
