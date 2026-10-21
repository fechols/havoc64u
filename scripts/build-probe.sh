#!/usr/bin/env bash
# Build the standalone hardware timing probe PRG for the C64 Ultimate.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
output=${1:-dist/hwprobe}
mkdir -p "$output"
output=$(cd "$output" && pwd)
(cd tools/hwprobe && cargo build --release)
cp tools/hwprobe/target/mos-c64-none/release/havoc-hwprobe "$output/hwprobe.prg"
printf 'Built %s/hwprobe.prg (%s bytes)\n' "$output" "$(stat -c %s "$output/hwprobe.prg")"
