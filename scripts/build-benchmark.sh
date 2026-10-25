#!/usr/bin/env bash
# Build a separately packaged, monitor-controlled benchmark executable.
set -euo pipefail
if (( $# > 2 )); then
    echo 'usage: scripts/build-benchmark.sh [output-directory] [painter|spans|front]' >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
renderer=${2:-painter}
native_features=benchmarks
default_output=target/bench/instrumented
native_target="$root/target/bench/cargo-native"
case "$renderer" in
    painter) ;;
    spans) native_features="$native_features,span-buffer"; native_target="$native_target-spans"; default_output="$default_output-spans" ;;
    front) native_features="$native_features,span-front-to-back"; native_target="$native_target-front"; default_output="$default_output-front" ;;
    *) echo 'renderer must be painter, spans or front' >&2; exit 2 ;;
esac
output=${1:-$default_output}
mkdir -p "$output"
output=$(cd "$output" && pwd)
cargo +stable run --release -- bake "$output"
# Keep instrumented and renderer-specific caches separate from production.
(cd target-c64 && CARGO_TARGET_DIR="$native_target" cargo build --release --features "$native_features")
native_prg="$native_target/mos-c64-none/release/havoc-c64"
# Cargo hard-links the selected hashed PRG into the top-level executable;
# select its sibling ELF by file identity, independent of cached build dates.
elf=
for candidate in "$native_target/mos-c64-none/release/deps"/havoc_c64-*.elf; do
    if [[ "${candidate%.elf}" -ef "$native_prg" ]]; then
        [[ -z "$elf" ]] || { echo "Ambiguous ELF for $native_prg; clean and rebuild $native_target" >&2; exit 1; }
        elf=$candidate
    fi
done
[[ -n "$elf" ]] || { echo "No matching ELF for $native_prg; clean and rebuild $native_target" >&2; exit 1; }
cp "$native_prg" "$output/havoc.prg"
cp "$elf" "$output/havoc.elf"
python3 tools/check-layout.py "$output/havoc.elf" "$output/havoc.prg" > "$output/memory-map.txt"
python3 tools/check-overlays.py "$output/havoc.elf"
python3 tools/test-reu-loader.py "$output/havoc.elf"
python3 tools/pack-overlays.py "$output/havoc.elf" "$output/havoc.prg" "$output/havoc.reu"
cargo +stable run --release -- check "$output/havoc.reu"
printf 'Built benchmark package: %s\n' "$output"
