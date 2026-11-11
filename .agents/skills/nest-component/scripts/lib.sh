#!/usr/bin/env bash
# Shared helpers of the nest-component skill scripts.
#
# Everything which depends on where NEST lives is resolved here, so that the
# other scripts never guess. Sources, do not execute.

# Directory of the NEST checkout, which provides the WIT definitions and, when
# built, the server used to verify a component. Override with NEST_REPO.
nest_repo() {
    if [[ -n "${NEST_REPO:-}" ]]; then
        printf '%s\n' "$NEST_REPO"
        return 0
    fi
    # SKILL_DIR if the caller set it, otherwise this file's own skill directory
    local dir="${SKILL_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
    while [[ "$dir" != "/" && -n "$dir" ]]; do
        if [[ -f "$dir/wit/nest-component.wit" && -d "$dir/crates/nest-server" ]]; then
            printf '%s\n' "$dir"
            return 0
        fi
        dir="$(dirname "$dir")"
    done
    return 1
}

# Path to the nest-server binary, or empty when it was not built
nest_server() {
    local repo
    repo="$(nest_repo)" || return 1
    for candidate in "$repo/target/release/nest-server" "$repo/target/debug/nest-server"; do
        [[ -x "$candidate" ]] && { printf '%s\n' "$candidate"; return 0; }
    done
    command -v nest-server 2>/dev/null
}

# The WIT files of the nest:plugin package, copied rather than fetched so that a
# component is always built against the checkout it was authored in
nest_wit() {
    local repo
    repo="$(nest_repo)" || return 1
    printf '%s\n' "$repo/wit"
}

# wasm-tools, from PATH or from the pinned nixpkgs package; prints the binary
# path, since calling it through a shell function makes argument passing fragile
wasm_tools_bin() {
    if command -v wasm-tools >/dev/null 2>&1; then
        printf 'wasm-tools\n'
        return 0
    fi
    local out
    out="$(nix build --no-link --print-out-paths --impure \
        --expr 'let p = import <nixpkgs> {}; in p.wasm-tools' 2>/dev/null | tail -n 1)"
    if [[ -n "$out" && -x "$out/bin/wasm-tools" ]]; then
        printf '%s\n' "$out/bin/wasm-tools"
        return 0
    fi
    return 1
}

# Run a Python script with the WebSocket client available
run_python() {
    uv run --no-project --quiet --with websocket-client python "$@"
}

free_port() {
    uv run --no-project --quiet python -c \
        'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

# The header comment of the calling script, which is where its usage is written
usage() {
    awk 'NR > 1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[1]}"
}

log() {
    printf '  %s\n' "$*" >&2
}
