#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
(cd target-c64 && cargo build --release)
mkdir -p dist
cp target-c64/target/mos-c64-none/release/sueme64-c64 dist/sueme64.prg
elf=$(find target-c64/target/mos-c64-none/release/deps -name 'sueme64_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" dist/sueme64.elf
python3 tools/check-layout.py dist/sueme64.elf dist/sueme64.prg > dist/memory-map.txt
python3 tools/check-overlays.py dist/sueme64.elf
cat dist/memory-map.txt
python3 tools/pack-overlays.py dist/sueme64.elf dist/sueme64.prg dist/sueme64.reu
