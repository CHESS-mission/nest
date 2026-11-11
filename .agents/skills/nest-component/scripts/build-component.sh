#!/usr/bin/env bash
# Build a component project into a WebAssembly component.
#
# Usage: build-component.sh [directory]
#
# The language and route come from .nest-component.json, written by new-component.sh.
# Prints the artifact path on success.

set -euo pipefail
SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib.sh
source "$SKILL_DIR/scripts/lib.sh"
[[ "${1:-}" == "--help" || "${1:-}" == "-h" ]] && usage && exit 0

directory="$(cd "${1:-.}" && pwd)"
manifest="$directory/.nest-component.json"
[[ -f "$manifest" ]] || die "$manifest not found; run new-component.sh first"

name="$(uv run --no-project --quiet python -c "import json,sys;print(json.load(open('$manifest'))['name'])")"
language="$(uv run --no-project --quiet python -c "import json,sys;print(json.load(open('$manifest'))['language'])")"
route="$(uv run --no-project --quiet python -c "import json,sys;print(json.load(open('$manifest'))['route'])")"
world="$(uv run --no-project --quiet python -c "import json,sys;print(json.load(open('$manifest'))['world'])")"

# Cargo replaces the dashes of a package name in the artifact it writes
cargo_name="${name//-/_}"

# C and C++ share a route. The difference is only which compiler takes the component
# file: the generated bindings are C, and must be compiled as C even when the component
# is not, because the component type object is referenced through a C symbol.
build_native() { # extension, compiler
    local extension="$1" compiler source
    [[ -n "${WASI_SDK_PATH:-}" ]] || die "WASI_SDK_PATH is unset; use nix develop .#component-c"
    compiler="$WASI_SDK_PATH/bin/$2"
    command -v wit-bindgen >/dev/null 2>&1 || die "wit-bindgen not found; use nix develop .#component-c"
    (cd "$directory" && wit-bindgen c wit --world "$world" --out-dir bindings 1>&2)

    # wit-bindgen names the files after the world, with dashes as underscores
    local bindings_name="${world##*/}"
    bindings_name="${bindings_name//-/_}"

    source="$directory/$name.$extension"
    [[ -f "$source" ]] || die "no $source; the scaffold writes $name.$extension"

    "$compiler" --target=wasm32-wasip2 -O2 -I. -c "$source" -o "$directory/build/$name.o" 1>&2
    "$WASI_SDK_PATH/bin/clang" --target=wasm32-wasip2 -O2 -I. \
        -c "$directory/bindings/$bindings_name.c" -o "$directory/build/bindings.o" 1>&2
    "$compiler" --target=wasm32-wasip2 -mexec-model=reactor \
        "$directory/build/$name.o" "$directory/build/bindings.o" \
        "$directory/bindings/${bindings_name}_component_type.o" \
        -o "$directory/build/$name.wasm" 1>&2
}

mkdir -p "$directory/build"

case "$language:$route" in
    rust:wasip2)
        (cd "$directory" && cargo build --target wasm32-wasip2 --release 1>&2)
        cp "$directory/target/wasm32-wasip2/release/$cargo_name.wasm" "$directory/build/$name.wasm"
        ;;

    rust:no-wasi)
        # The route which cannot import WASI at all: no adapter, so nothing to
        # trap on. Prefer it when the toolchain of the guide is unavailable.
        (cd "$directory" && cargo build --target wasm32-unknown-unknown --release 1>&2)
        wt="$(wasm_tools_bin)" || die "wasm-tools is needed to componentize this route"
        "$wt" component new \
            "$directory/target/wasm32-unknown-unknown/release/$cargo_name.wasm" \
            -o "$directory/build/$name.wasm" 1>&2
        ;;

    cpp:wasip2)
        build_native cpp clang++
        ;;

    c:wasip2)
        build_native c clang
        ;;

    python:*)
        # --stub-wasi is not an optimisation here: without it the interpreter reaches
        # for wasi:cli/environment at startup, and the host traps that
        command -v uv >/dev/null 2>&1 || die "uv not found; install uv, or use the rust route"
        (cd "$directory" && uv run --python 3.12 --with componentize-py \
            componentize-py -d wit -w "$world" componentize app --stub-wasi \
            -o "build/$name.wasm" 1>&2)
        ;;

    js:*)
        # -d all is the point: it strips every WASI feature out of the engine, so
        # that nothing the JavaScript runtime does at startup reaches the host
        command -v npm >/dev/null 2>&1 || die "npm not found; use nix develop .#component-js"
        if [[ ! -x "$directory/node_modules/.bin/jco" ]]; then
            (cd "$directory" && npm install --silent --no-save \
                @bytecodealliance/componentize-js 1>&2)
        fi
        (cd "$directory" && ./node_modules/.bin/jco componentize app.js --wit wit \
            --world-name "$world" -d all -o "build/$name.wasm" 1>&2)
        ;;

    *)
        die "unsupported language/route: $language/$route"
        ;;
esac

artifact="$directory/build/$name.wasm"
[[ -f "$artifact" ]] || die "the build produced no $artifact"

wt="$(wasm_tools_bin)" || die "wasm-tools is needed to inspect the artifact"
printf '%s (%s bytes)\n' "$artifact" "$(stat -c%s "$artifact")"
"$wt" component wit "$artifact" 2>&1 | grep -E "^  (export|import)" | sed 's/^/    /'
