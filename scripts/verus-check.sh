#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERUS_VERSION="${VERUS_VERSION:-0.2026.03.17.a96bad0}"
CACHE_BASE="${VERUS_CACHE_DIR:-$HOME/.cache/verus}"
INSTALL_ROOT="$CACHE_BASE/$VERUS_VERSION"

host_asset() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "verus-${VERUS_VERSION}-arm64-macos.zip" ;;
    Darwin-x86_64) echo "verus-${VERUS_VERSION}-x86-macos.zip" ;;
    Linux-x86_64) echo "verus-${VERUS_VERSION}-x86-linux.zip" ;;
    *)
      echo "unsupported host for Verus: $(uname -s)-$(uname -m)" >&2
      exit 1
      ;;
  esac
}

ensure_verus() {
  local asset zip_path url install_dir toolchain

  mkdir -p "$INSTALL_ROOT"
  install_dir="$(find "$INSTALL_ROOT" -maxdepth 1 -type d -name 'verus-*' | head -n 1 || true)"
  if [[ -z "$install_dir" || ! -x "$install_dir/cargo-verus" ]]; then
    asset="$(host_asset)"
    zip_path="$INSTALL_ROOT/$asset"
    url="https://github.com/verus-lang/verus/releases/download/release/${VERUS_VERSION}/${asset}"
    curl -fsSL -o "$zip_path" "$url"
    unzip -q -o "$zip_path" -d "$INSTALL_ROOT"
    install_dir="$(find "$INSTALL_ROOT" -maxdepth 1 -type d -name 'verus-*' | head -n 1 || true)"
  fi

  if [[ -z "$install_dir" || ! -f "$install_dir/version.json" ]]; then
    echo "failed to install Verus into $INSTALL_ROOT" >&2
    exit 1
  fi

  toolchain="$(python3 - <<'PY' "$install_dir/version.json"
import json
import pathlib
import sys

data = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["verus"]["toolchain"])
PY
)"

  if ! rustup toolchain list | grep -q "^${toolchain}\\b"; then
    rustup toolchain install "$toolchain"
  fi

  printf '%s\n' "$install_dir"
}

VERUS_DIR="$(ensure_verus)"
export PATH="$VERUS_DIR:$PATH"

cd "$ROOT_DIR"
cargo verus focus -p telomere "$@"
