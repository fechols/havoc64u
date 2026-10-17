#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
command -v mos-c64-clang >/dev/null || { echo 'Install LLVM-MOS SDK v22.5.0; see README.md' >&2; exit 1; }
rustup run mos rustc --version
cargo +stable test --all-targets
cargo +stable run --release -- bake dist
(cd target-c64 && cargo build --release)
cp target-c64/target/mos-c64-none/release/sueme64-c64 dist/sueme64.prg
elf=$(find target-c64/target/mos-c64-none/release/deps -name 'sueme64_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" dist/sueme64.elf
python3 tools/check-layout.py dist/sueme64.elf dist/sueme64.prg > dist/memory-map.txt
python3 tools/check-overlays.py dist/sueme64.elf
python3 tools/pack-overlays.py dist/sueme64.elf dist/sueme64.prg dist/sueme64.reu
cargo +stable run --release -- check dist/sueme64.reu
printf 'Built %s/dist/sueme64.{prg,reu,cfg}\n' "$root"
