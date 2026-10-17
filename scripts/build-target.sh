#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
profile=${1:-2sid}
features=()
case "$profile" in
    2sid) output=dist; native_target="$root/target-c64/target" ;;
    4sid) output=dist/4sid; native_target="$root/target-c64/target-4sid"; features=(--features four-sid) ;;
    *) echo 'usage: scripts/build-target.sh [2sid|4sid]' >&2; exit 2 ;;
esac
(cd target-c64 && CARGO_TARGET_DIR="$native_target" cargo build --release "${features[@]}")
mkdir -p "$output"
cp "$native_target/mos-c64-none/release/havoc-c64" "$output/havoc.prg"
elf=$(find "$native_target/mos-c64-none/release/deps" -name 'havoc_c64-*.elf' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)
cp "$elf" "$output/havoc.elf"
python3 tools/check-layout.py "$output/havoc.elf" "$output/havoc.prg" > "$output/memory-map.txt"
python3 tools/check-overlays.py "$output/havoc.elf"
cat "$output/memory-map.txt"
python3 tools/pack-overlays.py "$output/havoc.elf" "$output/havoc.prg" "$output/havoc.reu"
