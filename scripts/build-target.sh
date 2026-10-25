#!/usr/bin/env bash
set -euo pipefail
if (( $# != 0 )); then
    echo 'usage: scripts/build-target.sh (the four-UltiSID build has no sound-profile argument)' >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
python3 tools/encode-splash.py --verify-assets
output=dist
native_target="$root/target-c64/target"
(cd target-c64 && CARGO_TARGET_DIR="$native_target" cargo build --release)
native_prg="$native_target/mos-c64-none/release/havoc-c64"
# Cargo hard-links the selected hashed PRG into the top-level executable.
# Its sibling ELF remains the right one even when a newer feature build is cached.
elf=
for candidate in "$native_target/mos-c64-none/release/deps"/havoc_c64-*.elf; do
    if [[ "${candidate%.elf}" -ef "$native_prg" ]]; then
        [[ -z "$elf" ]] || { echo "Ambiguous ELF for $native_prg; clean and rebuild $native_target" >&2; exit 1; }
        elf=$candidate
    fi
done
[[ -n "$elf" ]] || { echo "No matching ELF for $native_prg; clean and rebuild $native_target" >&2; exit 1; }
mkdir -p "$output"
cp "$native_prg" "$output/havoc.prg"
cp "$elf" "$output/havoc.elf"
python3 tools/check-layout.py "$output/havoc.elf" "$output/havoc.prg" > "$output/memory-map.txt"
python3 tools/check-overlays.py "$output/havoc.elf"
python3 tools/check-irq-frames.py "$output/havoc.elf"
python3 tools/test-splash-startup.py "$output/havoc.elf" "$output/havoc.prg"
python3 tools/test-reu-loader.py "$output/havoc.elf"
cat "$output/memory-map.txt"
python3 tools/pack-overlays.py "$output/havoc.elf" "$output/havoc.prg" "$output/havoc.reu"
