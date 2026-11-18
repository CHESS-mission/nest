#!/usr/bin/env bash
# Create the skeleton of a NEST component, ready to build.
#
# Usage: new-component.sh [--language rust|c|cpp|python|js] [--route wasip2|no-wasi] <name> [directory]
#
# The WIT of the nest:plugin package is copied from the NEST checkout rather than
# fetched, so that the component is authored against the interfaces it will run
# beside. Nothing else is invented: the world starts with one I2C port.

set -euo pipefail
SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib.sh
source "$SKILL_DIR/scripts/lib.sh"

language=rust
route=wasip2
name=""
target=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --language) language="$2"; shift 2 ;;
        --route) route="$2"; shift 2 ;;
        --help) usage; exit 0 ;;
        -*) die "unknown option $1" ;;
        *)
            if [[ -z "$name" ]]; then name="$1"; else target="$1"; fi
            shift
            ;;
    esac
done

[[ -n "$name" ]] || die "usage: new-component.sh [--language rust|c|cpp|python|js] [--route wasip2|no-wasi] <name> [directory]"
[[ "$name" =~ ^[a-z][a-z0-9]*(-[a-z0-9]+)*$ ]] || \
    die "the component name must be kebab-case (letters, digits, dashes), got $name"
case "$language" in rust|c|cpp|python|js) ;; *) die "--language must be rust, c, cpp, python or js" ;; esac
case "$route" in wasip2|no-wasi) ;; *) die "--route must be wasip2 or no-wasi" ;; esac
# Python has exactly one route, and it is not the one the other languages use
if [[ "$language" = python ]]; then
    route=stub-wasi
fi
if [[ "$language" = js ]]; then
    route=no-wasi-features
fi

[[ -n "$target" ]] || target="$name"
[[ -e "$target" ]] && die "$target already exists"
mkdir -p "$target"
target="$(cd "$target" && pwd)"

struct="$(printf '%s' "$name" | sed -E 's/(^|-)([a-z0-9])/\U\2/g')"

wit_dir="$(nest_wit)" || die "no NEST checkout found; set NEST_REPO to one"
repo="$(nest_repo)"

subst() { # template, destination
    local template="$1" destination="$2"
    sed -e "s|@NAME@|$name|g" \
        -e "s|@NAME_WIT@|$name|g" \
        -e "s|@NAME_STRUCT@|$struct|g" \
        -e "s|@NAME_C@|${name//-/_}|g" \
        -e "s|@LANGUAGE@|$language|g" \
        -e "s|@ROUTE@|$route|g" \
        -e "s|@DATE@|$(date --iso-8601 2>/dev/null || date)|g" \
        -e "s|@NEST_REV@|$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || echo unknown)|g" \
        -e "s|@TOOLCHAIN@|$language/$route|g" \
        -e "s|@ONE_LINE_DESCRIPTION@|TODO: one sentence saying what this models.|g" \
        -e "s|@DATASHEET@|TODO: part number, manufacturer and revision.|g" \
        "$SKILL_DIR/assets/$(basename "$template")" > "$destination"
}

mkdir -p "$target/wit/deps/nest-plugin"
cp "$wit_dir"/*.wit "$target/wit/deps/nest-plugin/"
subst world.wit.tmpl "$target/wit/$name.wit"
subst README.md.tmpl "$target/README.md"

case "$language" in
    rust)
        mkdir -p "$target/src"
        subst Cargo.toml.tmpl "$target/Cargo.toml"
        subst lib.rs.tmpl "$target/src/lib.rs"
        ;;
    c)
        subst component.c.tmpl "$target/$name.c"
        ;;
    cpp)
        subst component.c.tmpl "$target/$name.cpp"
        ;;
    python)
        # The module is called `app`, since Python would shadow the world otherwise
        subst app.py.tmpl "$target/app.py"
        ;;
    js)
        subst app.js.tmpl "$target/app.js"
        ;;
esac

cat > "$target/.nest-component.json" <<EOF
{
  "name": "$name",
  "world": "example:components/$name",
  "language": "$language",
  "route": "$route",
  "artifact": "build/$name.wasm",
  "repo": "$repo"
}
EOF

echo "created $target"
echo
echo "next:"
echo "  1. fill the register table of README.md from the datasheet"
echo "  2. edit wit/$name.wit so that its ports and reads match the part"
case "$language" in
    rust) echo "  3. implement src/lib.rs" ;;
    c) echo "  3. implement $name.c" ;;
    cpp) echo "  3. implement $name.cpp" ;;
    python) echo "  3. implement app.py" ;;
    js) echo "  3. implement app.js" ;;
esac
echo "  4. $SKILL_DIR/scripts/build-component.sh $target"
echo "  5. $SKILL_DIR/scripts/verify.sh $target   # must be green before you report success"
