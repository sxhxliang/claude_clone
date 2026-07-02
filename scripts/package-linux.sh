#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(python3 - <<'PY'
from pathlib import Path
for line in Path("Cargo.toml").read_text(encoding="utf-8").splitlines():
    line = line.strip()
    if line.startswith("version"):
        print(line.split("=", 1)[1].strip().strip('"'))
        break
PY
)"
OUT_DIR="${1:-dist}"
TARGET="${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
ARCH="${TARGET%%-*}"
ASSET="claude_clone-v${VERSION}-linux-${ARCH}.tar.gz"
WORK_DIR="${OUT_DIR}/linux-${TARGET}"

rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR" "$OUT_DIR"

cargo build --release
cp "target/release/claude_clone" "$WORK_DIR/claude_clone"
chmod +x "$WORK_DIR/claude_clone"
tar -czf "${OUT_DIR}/${ASSET}" -C "$WORK_DIR" claude_clone

echo "${OUT_DIR}/${ASSET}"
