#!/usr/bin/env bash
set -euo pipefail

REPO_OWNER="${GITEE_OWNER:-ynnyh}"
REPO_NAME="${GITEE_REPO:-jarvis}"
TAG="${JARVIS_VERSION:-}"
APP_NAME="${APP_NAME:-Jarvis}"

if [[ -z "$TAG" ]]; then
  echo "==> Reading latest version"
  LATEST_JSON="$(curl -fsSL "https://gitee.com/${REPO_OWNER}/${REPO_NAME}/raw/main/latest.json")"
  VERSION="$(printf '%s' "$LATEST_JSON" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [[ -z "$VERSION" ]]; then
    echo "Could not read latest version from latest.json" >&2
    exit 1
  fi
  TAG="v${VERSION}"
fi

RELEASE_URL="https://gitee.com/${REPO_OWNER}/${REPO_NAME}/releases/tag/${TAG}"

echo "==> Looking up ${APP_NAME} macOS DMG from ${RELEASE_URL}"
HTML="$(curl -fsSL "$RELEASE_URL")"
DMG_PATH="$(printf '%s' "$HTML" \
  | grep -Eo "/${REPO_OWNER}/${REPO_NAME}/releases/download/[^\"']+\\.dmg" \
  | head -n 1 || true)"

if [[ -z "$DMG_PATH" ]]; then
  echo "Could not find a .dmg asset on ${RELEASE_URL}" >&2
  exit 1
fi

DMG_URL="https://gitee.com${DMG_PATH}"
WORKDIR="$(mktemp -d)"
DMG_FILE="${WORKDIR}/${APP_NAME}.dmg"
MOUNT_DIR="${WORKDIR}/mnt"

cleanup() {
  hdiutil detach "$MOUNT_DIR" -quiet >/dev/null 2>&1 || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "==> Downloading ${DMG_URL}"
curl -fL "$DMG_URL" -o "$DMG_FILE"

mkdir -p "$MOUNT_DIR"
echo "==> Mounting DMG"
hdiutil attach "$DMG_FILE" -mountpoint "$MOUNT_DIR" -nobrowse -quiet

APP_SOURCE="$(find "$MOUNT_DIR" -maxdepth 2 -name "${APP_NAME}.app" -type d | head -n 1)"
if [[ -z "$APP_SOURCE" ]]; then
  echo "Could not find ${APP_NAME}.app in DMG" >&2
  exit 1
fi

APP_TARGET="/Applications/${APP_NAME}.app"
echo "==> Installing to ${APP_TARGET}"
rm -rf "$APP_TARGET"
cp -R "$APP_SOURCE" "$APP_TARGET"

echo "==> Removing Gatekeeper quarantine attribute for internal dev build"
xattr -dr com.apple.quarantine "$APP_TARGET" 2>/dev/null || true

echo "==> Verifying architecture"
file "$APP_TARGET/Contents/MacOS/"* || true

echo "==> Opening ${APP_NAME}"
open "$APP_TARGET"

echo "Done. If macOS still blocks it, run:"
echo "  xattr -dr com.apple.quarantine '${APP_TARGET}'"
