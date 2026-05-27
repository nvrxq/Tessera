#!/usr/bin/env bash
# Tessera one-liner installer. Fetches the latest GitHub release for the
# host OS+arch and installs it to /Applications (macOS) or ~/.local/bin
# (Linux). Idempotent — re-running upgrades in place.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/nvrxq/Tessera/main/install.sh | sh
#
# Env overrides:
#   TESSERA_REPO     — override the GitHub repo (default: nvrxq/Tessera)
#   TESSERA_VERSION  — install a specific tag (default: latest)

set -euo pipefail

REPO="${TESSERA_REPO:-nvrxq/Tessera}"
VERSION="${TESSERA_VERSION:-latest}"

bold()  { printf "\033[1m%s\033[0m\n" "$*"; }
red()   { printf "\033[31m%s\033[0m\n" "$*"; }
warn()  { printf "\033[33m%s\033[0m\n" "$*"; }

need() {
  command -v "$1" >/dev/null 2>&1 || { red "missing prerequisite: $1"; exit 1; }
}

need curl
need uname

OS="$(uname -s)"
ARCH="$(uname -m)"

# Map OS+arch to the asset suffix tauri-action publishes. Pattern is anchored
# to end-of-name so e.g. *.dmg.sig doesn't collide with *.dmg.
case "$OS-$ARCH" in
  Darwin-arm64)   ASSET_RE='aarch64\.dmg$'        ;;
  Darwin-x86_64)
    red "Intel Mac builds aren't published — please build from source:"
    echo "  git clone https://github.com/$REPO.git && cd Tessera && ./scripts/build.sh"
    exit 1
    ;;
  Linux-x86_64)   ASSET_RE='amd64\.AppImage$'     ;;
  Linux-aarch64)  ASSET_RE='aarch64\.AppImage$'   ;;
  *)              red "unsupported platform: $OS-$ARCH"; exit 1 ;;
esac

bold "==> resolving release"
if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/$REPO/releases/latest"
else
  API="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
fi

# Find the first asset URL matching ASSET_RE. GitHub's API JSON exposes
# `browser_download_url` for each asset; grep keeps this script jq-free.
RELEASE_JSON="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API")"
ASSET_URL="$(printf '%s' "$RELEASE_JSON" \
  | grep -Eo 'https://[^"]+' \
  | grep -E "$ASSET_RE" \
  | head -n1 || true)"

if [ -z "$ASSET_URL" ]; then
  red "no asset matching /$ASSET_RE/ in release '$VERSION' of $REPO"
  red "available assets:"
  printf '%s' "$RELEASE_JSON" | grep -Eo 'https://[^"]+\.(dmg|AppImage|deb|tar\.gz)' | sed 's/^/  /'
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
NAME="$(basename "$ASSET_URL")"

bold "==> downloading $NAME"
curl -#fL -o "$TMP/$NAME" "$ASSET_URL"

case "$OS" in
  Darwin)
    bold "==> mounting $NAME"
    # `hdiutil attach -plist` is the parsing-friendly form; the line-grep
    # below works on every macOS since 10.4 without xmllint.
    MOUNT_DEV_AND_POINT="$(hdiutil attach -nobrowse -noautoopen "$TMP/$NAME" \
      | grep -E '\sApple_HFS\s|\sapfs\s' \
      | tail -n1)"
    if [ -z "$MOUNT_DEV_AND_POINT" ]; then
      red "could not parse hdiutil attach output"
      exit 1
    fi
    MOUNT_POINT="$(printf '%s' "$MOUNT_DEV_AND_POINT" | awk '{for (i=3; i<=NF; i++) printf "%s%s", $i, (i==NF?"":" ")}')"
    APP_SRC="$(find "$MOUNT_POINT" -maxdepth 2 -name '*.app' -print -quit)"
    if [ -z "$APP_SRC" ]; then
      red "no .app inside the dmg"
      hdiutil detach "$MOUNT_POINT" >/dev/null || true
      exit 1
    fi
    APP_NAME="$(basename "$APP_SRC")"
    DEST="/Applications/$APP_NAME"
    bold "==> installing to $DEST"
    if [ -d "$DEST" ]; then
      rm -rf "$DEST"
    fi
    if cp -R "$APP_SRC" "$DEST" 2>/dev/null; then
      :
    else
      warn "cp to /Applications failed — retrying with sudo"
      sudo cp -R "$APP_SRC" "$DEST"
    fi
    hdiutil detach "$MOUNT_POINT" >/dev/null
    # The bundle was downloaded from the internet → Gatekeeper has flagged
    # it with the quarantine xattr. Since we're not Apple-notarised, strip
    # it ourselves; the user already opted in by running this script.
    xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
    # Re-sign ad-hoc so TCC has a deterministic signature to key off — at
    # least permission grants survive across reinstalls of the same release.
    # Real "permissions persist across updates" requires Apple Developer ID
    # signing + notarisation, which we don't have.
    codesign --force --deep --sign - --options runtime "$DEST" 2>/dev/null || true
    echo
    bold "==> done"
    echo "Launch: open '$DEST'"
    echo "Note: macOS will still prompt for permissions on first access to"
    echo "      protected folders (Documents/Downloads/Music/Pictures) and"
    echo "      to read clipboard images — that's a one-time cost per"
    echo "      category. Without an Apple Developer ID we can't make"
    echo "      grants survive across binary updates."
    ;;

  Linux)
    DEST="$HOME/.local/bin/tessera"
    bold "==> installing to $DEST"
    mkdir -p "$(dirname "$DEST")"
    install -m 755 "$TMP/$NAME" "$DEST"
    echo
    bold "==> done"
    echo "Launch: tessera"
    case ":$PATH:" in
      *":$HOME/.local/bin:"*) : ;;
      *) warn "$HOME/.local/bin is not on PATH — add it to your shell rc." ;;
    esac
    ;;
esac
