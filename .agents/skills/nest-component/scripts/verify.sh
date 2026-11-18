#!/usr/bin/env bash
# Run verify.py with the two Python packages it needs.
#
# Usage: verify.sh [<component-directory>] [--wasm <artifact>] [--no-build]

set -euo pipefail
SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export SKILL_DIR
exec uv run --no-project --quiet \
    --with pyyaml \
    --with websocket-client \
    python "$SKILL_DIR/scripts/verify.py" "$@"
