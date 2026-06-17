#!/usr/bin/env bash
#
# Cross-compile for Raspberry Pi 4 (aarch64) using cross-rs
#
# Prerequisites:
#   cargo install cross --git https://github.com/cross-rs/cross
#   Docker must be running
#
# Usage:
#   ./scripts/build-rpi4.sh          # Release build
#   ./scripts/build-rpi4.sh debug    # Debug build
#

set -euo pipefail

TARGET="aarch64-unknown-linux-gnu"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$PROJECT_ROOT"

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo "Error: 'cross' is not installed."
    echo "Install with: cargo install cross --git https://github.com/cross-rs/cross"
    exit 1
fi

# Check if Docker is running
if ! docker info &> /dev/null; then
    echo "Error: Docker is not running."
    exit 1
fi

# Compute the git build id on the host and forward it into the cross
# container (allow-listed in Cross.toml). build.rs reads $GIT_DESCRIBE;
# the container has no git history of its own. Empty is fine — build.rs
# falls back to the crate version.
GIT_DESCRIBE="$(git describe --tags --always --dirty 2>/dev/null || true)"
export GIT_DESCRIBE

# Determine build mode
BUILD_MODE="${1:-release}"

if [[ "$BUILD_MODE" == "debug" ]]; then
    echo "Building DEBUG for $TARGET..."
    cross build --target "$TARGET"
    BINARY="target/$TARGET/debug/duplo-train-controller"
else
    echo "Building RELEASE for $TARGET..."
    cross build --release --target "$TARGET"
    BINARY="target/$TARGET/release/duplo-train-controller"
fi

# Show result
if [[ -f "$BINARY" ]]; then
    echo ""
    echo "Build successful!"
    echo "Binary: $BINARY"
    echo "Size: $(du -h "$BINARY" | cut -f1)"
    echo ""
    echo "Copy to RPi4:"
    echo "  scp $BINARY pi@<rpi-ip>:~/"
else
    echo "Error: Binary not found at $BINARY"
    exit 1
fi
