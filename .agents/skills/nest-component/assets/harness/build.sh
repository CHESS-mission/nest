#!/usr/bin/env bash
# Rebuild harness.wasm from the sources next to this file.
#
# Run this after a change to the WIT of the nest:plugin package, since the
# harness is bound by name to the same interfaces as any component.
#
# Usage: build.sh [--route wasip2|no-wasi]

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill="$(cd "$here/../.." && pwd)"
# shellcheck source=../../scripts/lib.sh
source "$skill/scripts/lib.sh"

route="no-wasi"
[[ "${1:-}" == "--route" ]] && route="$2"

wit="$(nest_wit)" || die "no NEST checkout found; set NEST_REPO"
mkdir -p "$here/wit/deps/nest-plugin"
cp "$wit"/*.wit "$here/wit/deps/nest-plugin/"

case "$route" in
    no-wasi)
        (cd "$here" && cargo build --target wasm32-unknown-unknown --release)
        wt="$(wasm_tools_bin)" || die "wasm-tools not found"
        "$wt" component new "$here/target/wasm32-unknown-unknown/release/harness.wasm" \
            -o "$here/harness.wasm"
        ;;
    wasip2)
        (cd "$here" && cargo build --target wasm32-wasip2 --release)
        cp "$here/target/wasm32-wasip2/release/harness.wasm" "$here/harness.wasm"
        ;;
    *)
        die "--route must be no-wasi or wasip2"
        ;;
esac

printf 'harness.wasm: %s bytes\n' "$(stat -c%s "$here/harness.wasm")"
