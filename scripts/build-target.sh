#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
(cd target-c64 && cargo build --release)
mkdir -p dist
cp target-c64/target/mos-c64-none/release/havoc-c64 dist/havoc.prg
elf=$(find target-c64/target/mos-c64-none/release/deps -name 'havoc_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" dist/havoc.elf
python3 tools/check-layout.py dist/havoc.elf dist/havoc.prg > dist/memory-map.txt
python3 tools/check-overlays.py dist/havoc.elf
cat dist/memory-map.txt
python3 tools/pack-overlays.py dist/havoc.elf dist/havoc.prg dist/havoc.reu
