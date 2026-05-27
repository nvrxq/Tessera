#!/usr/bin/env bash
# Build Tessera in release mode and produce a single self-contained binary.
# Works on macOS and Linux. No tauri CLI required.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"

bold()  { printf "\033[1m%s\033[0m\n" "$*"; }
red()   { printf "\033[31m%s\033[0m\n" "$*"; }

need() {
  command -v "$1" >/dev/null 2>&1 || { red "missing: $1"; echo "$2"; exit 1; }
}

bold "==> checking prerequisites"
need cargo  "install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
need bun    "install Bun:  curl -fsSL https://bun.sh/install | bash"

if ! command -v claude >/dev/null 2>&1; then
  echo "warning: 'claude' not on PATH. Install Claude Code:"
  echo "         npm install -g @anthropic-ai/claude-code"
  echo "         (Tessera will still build, but you can't spawn agents without it.)"
fi

case "$(uname -s)" in
  Linux*)
    bold "==> Linux: system deps (webkit2gtk etc.)"
    echo "Need these once via apt (or your distro's pkg manager):"
    echo "  sudo apt-get install libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \\"
    echo "                       libayatana-appindicator3-dev librsvg2-dev \\"
    echo "                       libsoup-3.0-dev libjavascriptcoregtk-4.1-dev patchelf"
    ;;
  Darwin*)
    bold "==> macOS: using system WebKit, no extra system deps needed"
    ;;
esac

bold "==> building UI (bun)"
(cd ui && bun install && bun run build)

bold "==> building release binary (cargo)"
cargo build --release --bin tessera --features custom-protocol

BIN="$ROOT/target/release/tessera"

# Ad-hoc codesign on macOS so TCC's signature-based identity is stable
# across no-op rebuilds. Without ANY signature (or with -dev's default
# linker-supplied weak one) macOS re-prompts for Photos/Documents/etc
# every launch. Real fix is Apple Developer ID + notarization; this is
# the cheap stopgap.
if [ "$(uname -s)" = "Darwin" ]; then
  bold "==> ad-hoc codesigning"
  codesign --force --sign - --options runtime "$BIN" 2>&1 | sed 's/^/    /' || true
fi

SIZE="$(du -h "$BIN" | cut -f1)"

echo
bold "==> done"
echo "Binary: $BIN"
echo "Size:   $SIZE"
echo
echo "Launch:"
echo "  $BIN"
echo
echo "Optional — put a copy in your PATH:"
echo "  cp \"$BIN\" \"\$HOME/.local/bin/tessera\""
