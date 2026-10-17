#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
source scripts/env.sh
command -v mos-c64-clang >/dev/null || { echo 'Install LLVM-MOS SDK v22.5.0; see README.md' >&2; exit 1; }
rustup run mos rustc --version
cargo +stable test --all-targets
cargo +stable test --all-targets --features four-sid
for profile in 2sid 4sid; do
    features=()
    output=dist
    if [[ "$profile" == 4sid ]]; then
        features=(--features four-sid)
        output=dist/4sid
    fi
    cargo +stable run --release "${features[@]}" -- bake "$output"
    bash scripts/build-target.sh "$profile"
    cargo +stable run --release "${features[@]}" -- check "$output/havoc.reu"
    printf 'Built %s/%s/havoc.{prg,reu,cfg}\n' "$root" "$output"
done
