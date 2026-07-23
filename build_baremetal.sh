#!/bin/bash
set -e

# Build Zohara kernel for bare metal with multiboot header.
# Uses custom linker script that places .multiboot within first 8KB.
# NOT suitable for QEMU -kernel (QEMU rejects 64-bit ELFs with multiboot header).

echo "Building kernel for bare metal (with multiboot header)..."
cargo rustc --target x86_64-unknown-none --release \
    -- \
    -C link-arg=-Tlink.ld \
    2>&1 | grep -E "error|warning:.*link|Finished" || true

BIN="target/x86_64-unknown-none/release/zohara"

if [ ! -f "$BIN" ]; then
    echo "ERROR: Build failed"
    exit 1
fi

echo "Verifying multiboot header..."
if grub-file --is-x86-multiboot "$BIN" 2>/dev/null; then
    echo "  multiboot1: VALID"
else
    echo "  multiboot1: INVALID"
    exit 1
fi

if grub-file --is-x86-multiboot2 "$BIN" 2>/dev/null; then
    echo "  multiboot2: VALID"
else
    echo "  multiboot2: INVALID (expected — Zohara uses multiboot1)"
fi

DEPLOY="/boot/zohara.bin"
echo "Deploying to $DEPLOY..."
echo "admin144" | sudo -S cp "$BIN" "$DEPLOY"
echo "admin144" | sudo -S chmod 644 "$DEPLOY"

echo "Done. Kernel deployed to $DEPLOY"
echo "Run 'sudo update-grub' to add Zohara entry to GRUB menu."
