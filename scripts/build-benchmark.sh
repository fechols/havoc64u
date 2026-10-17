#!/usr/bin/env bash
# Build a separately packaged, monitor-controlled benchmark executable.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
output=${1:-target/bench/instrumented}
mkdir -p "$output"
output=$(cd "$output" && pwd)
cargo +stable run --release -- bake "$output"
# Separate feature artifacts prevent a later production package from selecting
# a newer benchmark ELF when Cargo reuses the normal release build.
native_target="$root/target/bench/cargo-native"
(cd target-c64 && CARGO_TARGET_DIR="$native_target" cargo build --release --features benchmarks)
cp "$native_target/mos-c64-none/release/havoc-c64" "$output/havoc.prg"
elf=$(find "$native_target/mos-c64-none/release/deps" -name 'havoc_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" "$output/havoc.elf"
python3 tools/check-layout.py "$output/havoc.elf" "$output/havoc.prg" > "$output/memory-map.txt"
python3 tools/check-overlays.py "$output/havoc.elf"
python3 tools/pack-overlays.py "$output/havoc.elf" "$output/havoc.prg" "$output/havoc.reu"
printf 'Built benchmark package: %s\n' "$output"
