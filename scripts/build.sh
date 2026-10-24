#!/usr/bin/env bash
set -euo pipefail
if (( $# != 0 )); then
    echo 'usage: scripts/build.sh' >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
python3 tools/digital_format.py
command -v mos-c64-clang >/dev/null || { echo 'Install LLVM-MOS SDK v22.5.0; see README.md' >&2; exit 1; }
rustup run mos rustc --version
cargo +stable test --all-targets
python3 tools/test-package-release.py
python3 tools/test-sfx-profile.py
python3 tools/test-bench-sfx.py
cargo +stable run --release -- bake dist
bash scripts/build-target.sh
cargo +stable run --release -- check dist/havoc.reu
python3 tools/package-release.py
printf 'Built %s/dist/havoc.{prg,reu,cfg}\n' "$root"
