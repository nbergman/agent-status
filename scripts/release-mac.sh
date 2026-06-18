#!/usr/bin/env bash
#
# Build, sign (Developer ID), and notarize the macOS .app + .dmg.
#
# Prerequisites:
#   - A "Developer ID Application" cert in your login keychain
#   - Notarization credentials (see .env.example / docs/RELEASE.md)
#
# Usage:
#   cp .env.example .env   # then fill in your credentials
#   ./scripts/release-mac.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Load local credentials if present (.env is gitignored).
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  echo "error: APPLE_SIGNING_IDENTITY is not set. Copy .env.example to .env and fill it in." >&2
  echo "       List your identities with: security find-identity -v -p codesigning" >&2
  exit 1
fi

# Notarization needs EITHER Apple ID creds OR an API key. Warn (don't fail) if
# neither is present so you can still produce a signed-but-unnotarized build.
notarize=true
if [[ -z "${APPLE_API_KEY:-}" && ( -z "${APPLE_ID:-}" || -z "${APPLE_PASSWORD:-}" ) ]]; then
  notarize=false
  echo "warning: no notarization credentials found — building SIGNED but NOT notarized." >&2
  echo "         The DMG will trigger Gatekeeper warnings on other Macs." >&2
fi

# Universal (Intel + Apple Silicon) by default. Set TARGET=aarch64-apple-darwin
# (or x86_64-apple-darwin) to build host-native / single-arch instead.
TARGET="${TARGET:-universal-apple-darwin}"

echo "==> Signing identity: ${APPLE_SIGNING_IDENTITY}"
echo "==> Notarization: $([[ "$notarize" == true ]] && echo enabled || echo disabled)"
echo "==> Target: ${TARGET}"
echo

# Tauri signs with the hardened runtime when APPLE_SIGNING_IDENTITY is set,
# notarizes + staples automatically when notarization creds are present, and
# produces the updater artifacts (.app.tar.gz + .sig) when an updater signing
# key is set in the environment.
npx tauri build --target "$TARGET" --bundles app,dmg

# Bundles live under target/<triple>/release when --target is passed.
APP="src-tauri/target/${TARGET}/release/bundle/macos/Agent Usage Monitor.app"
DMG_DIR="src-tauri/target/${TARGET}/release/bundle/dmg"

echo
echo "==> Verifying code signature"
codesign --verify --deep --strict --verbose=2 "$APP"

echo
echo "==> Gatekeeper assessment (must say: accepted / source=Notarized Developer ID)"
spctl --assess --type execute --verbose=2 "$APP" || {
  echo "spctl rejected the app — check signing/notarization above." >&2
}

if [[ "$notarize" == true ]]; then
  echo
  echo "==> Verifying notarization ticket is stapled"
  xcrun stapler validate "$APP" || echo "stapler validate failed — see docs/RELEASE.md" >&2
fi

echo
echo "==> Done. Artifacts:"
ls -1 "$DMG_DIR"/*.dmg 2>/dev/null || true
ls -1d "$APP" 2>/dev/null || true
echo
echo "To publish an auto-update, upload these to the GitHub release tagged v<version>:"
echo "  - the .dmg (human download)"
echo "  - the .app.tar.gz + .app.tar.gz.sig (updater payload)"
echo "  - a latest.json manifest (see docs/RELEASE.md §6)"
