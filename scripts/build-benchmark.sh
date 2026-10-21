#!/usr/bin/env bash
# Build a separately packaged, monitor-controlled benchmark executable.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
profile=${2:-2sid}
renderer=${3:-painter}
features=()
native_features=benchmarks
case "$profile" in
    2sid) default_output=target/bench/instrumented; native_target="$root/target/bench/cargo-native" ;;
    4sid) default_output=target/bench/instrumented-4sid; native_target="$root/target/bench/cargo-native-4sid"; features=(--features four-sid); native_features=benchmarks,four-sid ;;
    *) echo 'usage: scripts/build-benchmark.sh [output-directory] [2sid|4sid] [painter|spans|front]' >&2; exit 2 ;;
esac
case "$renderer" in
    painter) ;;
    spans) native_features="$native_features,span-buffer"; native_target="$native_target-spans"; default_output="$default_output-spans" ;;
    front) native_features="$native_features,span-front-to-back"; native_target="$native_target-front"; default_output="$default_output-front" ;;
    *) echo 'renderer must be painter, spans or front' >&2; exit 2 ;;
esac
output=${1:-$default_output}
mkdir -p "$output"
output=$(cd "$output" && pwd)
cargo +stable run --release "${features[@]}" -- bake "$output"
# Separate feature artifacts prevent a later production package from selecting
# a newer benchmark ELF when Cargo reuses the normal release build.
(cd target-c64 && CARGO_TARGET_DIR="$native_target" cargo build --release --features "$native_features")
cp "$native_target/mos-c64-none/release/havoc-c64" "$output/havoc.prg"
elf=$(find "$native_target/mos-c64-none/release/deps" -name 'havoc_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" "$output/havoc.elf"
python3 tools/check-layout.py "$output/havoc.elf" "$output/havoc.prg" > "$output/memory-map.txt"
python3 tools/check-overlays.py "$output/havoc.elf"
python3 tools/pack-overlays.py "$output/havoc.elf" "$output/havoc.prg" "$output/havoc.reu"
cargo +stable run --release "${features[@]}" -- check "$output/havoc.reu"
printf 'Built benchmark package: %s\n' "$output"
