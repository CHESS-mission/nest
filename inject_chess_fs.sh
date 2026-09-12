#!/usr/bin/env bash
set -e

# Configuration
IMAGE="image/disk.qcow2"
BINARY="../CHESS-FS/build-fprime-automatic-arm-hf-linux/bin/arm-hf-linux/CHESS_Pathfinder0FS"
TARGET_DIR="/CHESS"
TARGET_FILE="${TARGET_DIR}/CHESS_Pathfinder0FS"

# Verify files exist
if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "Current directory: $(pwd)"
    exit 1
fi

if [ ! -f "$IMAGE" ]; then
    echo "Error: Image not found at $IMAGE"
    echo "Current directory: $(pwd)"
    exit 1
fi

echo "Injecting $BINARY into $IMAGE..."

# Run guestfish with automatic mounting (-i).
# If guestfish is already in PATH (i.e. we are inside the nix devShell) use it
# directly avoids downloading a second copy of libguestfs-with-appliance.
# Otherwise fall back to 'nix shell' for standalone usage.
_inject_into() {
    local img="$1"
    guestfish -a "$img" -i <<EOF
mkdir-p "$TARGET_DIR"
upload "$BINARY" "$TARGET_FILE"
chmod 0755 "$TARGET_FILE"
mkdir-p /root/bin
upload "$BINARY" /root/bin/CHESS_Pathfinder0FS_A
chmod 0755 /root/bin/CHESS_Pathfinder0FS_A
ln-sf /root/bin/CHESS_Pathfinder0FS_A /root/bin/stable
echo "Injected $img"
EOF
}

_inject() {
    _inject_into "$IMAGE"
    # Also inject into image-copy (used by NEST SEU topology)
    if [[ -f "image-copy/disk.qcow2" ]]; then
        _inject_into "image-copy/disk.qcow2"
    fi
}

if command -v guestfish &>/dev/null; then
    _inject
else
    nix shell nixpkgs#libguestfs-with-appliance --command bash -c "$(declare -f _inject); IMAGE='$IMAGE' BINARY='$BINARY' TARGET_DIR='$TARGET_DIR' TARGET_FILE='$TARGET_FILE' _inject"
fi

echo "Done."