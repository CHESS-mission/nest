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

# Run guestfish with automatic mounting (-i)
# We use 'nix shell' to ensure guestfish is available
nix shell nixpkgs#libguestfs-with-appliance --command guestfish -a "$IMAGE" -i <<EOF
mkdir-p "$TARGET_DIR"
upload "$BINARY" "$TARGET_FILE"
chmod 0755 "$TARGET_FILE"
echo "Successfully injected to $TARGET_FILE"
EOF

echo "Done."