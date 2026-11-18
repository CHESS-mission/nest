#!/usr/bin/env bash
# Report which component build routes the current environment can run.
#
# Run this before anything else: it says whether a build route is available, and
# therefore whether to continue or to ask for a shell. Never installs anything.

set -euo pipefail
SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib.sh
source "$SKILL_DIR/scripts/lib.sh"

check() { # name, command...
    local name="$1"
    shift
    if command -v "$1" >/dev/null 2>&1; then
        printf '  %-28s %s\n' "$name" "$("$@" 2>&1 | head -n 1)"
        return 0
    fi
    printf '  %-28s missing\n' "$name"
    return 1
}

echo "NEST"
repo="$(nest_repo)" && echo "  checkout: $repo" || echo "  checkout: not found (set NEST_REPO)"
if [[ -n "${repo:-}" ]]; then
    server="$(nest_server)" && echo "  server:   $server" || echo "  server:   not built (cargo build --release -p nest-server)"
    echo "  WIT:      $(nest_wit)"
fi

echo
echo "Toolchains"
have_wasm_tools=0
if wt="$(wasm_tools_bin)"; then
    have_wasm_tools=1
    printf '  %-28s %s\n' "wasm-tools" "$($wt --version | head -n 1)"
else
    printf '  %-28s missing (nor an argument-less nix build of it)\n' "wasm-tools"
fi
check "uv" uv --version || true
check "wit-bindgen" wit-bindgen --version || true

echo
echo "Rust"
targets=""
if check "cargo" cargo --version; then
    sysroot="$(rustc --print sysroot 2>/dev/null)"
    targets="$(ls "$sysroot/lib/rustlib" 2>/dev/null | tr '\n' ' ')"
    for target in wasm32-wasip2 wasm32-unknown-unknown; do
        if [[ -d "$sysroot/lib/rustlib/$target" ]]; then
            printf '  %-28s present\n' "target $target"
        else
            printf '  %-28s absent\n' "target $target"
        fi
    done
fi

echo
echo "C/C++"
if [[ -n "${WASI_SDK_PATH:-}" && -x "$WASI_SDK_PATH/bin/clang" ]]; then
    printf '  %-28s %s\n' "wasi-sdk" "$("$WASI_SDK_PATH/bin/clang" --version | head -n 1)"
else
    printf '  %-28s absent (WASI_SDK_PATH unset)\n' "wasi-sdk"
fi

echo
echo "Routes available here"

route() { printf '  %-13s %s\n' "$1" "$2"; }

if [[ -n "${repo:-}" && -x "${repo}/target/release/nest-server" ]]; then
    route verify  "yes  (verify.sh runs against any built artifact)"
else
    route verify   "no   (cargo build --release -p nest-server)"
fi

if [[ "$have_wasm_tools" = 1 && "$targets" == *wasm32-unknown-unknown* ]]; then
    route rust-no-wasi "yes  (wasm32-unknown-unknown, then wasm-tools component new)"
else
    route rust-no-wasi  "no   (needs the target and wasm-tools)"
fi

if [[ "$targets" == *wasm32-wasip2* ]]; then
    route rust-wasip2  "yes  (the route of the guide)"
else
    route rust-wasip2   "no   (nix develop .#component-rust, or rustup target add wasm32-wasip2)"
fi

if [[ -n "${WASI_SDK_PATH:-}" && -x "$WASI_SDK_PATH/bin/clang" ]]; then
    route c-cpp "yes  (wasi-sdk, wit-bindgen c)"
else
    route c-cpp  "no   (nix develop .#component-c)"
fi

if command -v node >/dev/null 2>&1; then
    route js "yes  (componentize-js with -d all; npm fetches it once)"
else
    route js  "no   (nix develop .#component-js)"
fi

if command -v uv >/dev/null 2>&1; then
    route python "yes  (componentize-py with --stub-wasi, fetched on first build)"
else
    route python  "no   (nix develop .#component-python, or install uv)"
fi
