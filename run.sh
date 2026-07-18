#!/bin/bash
# Usage: ./run.sh [OPTIONS]
#   -t, --tasks N       Number of tasks to spawn (default: 0 = none)
#   -d, --debug         Enable verbose debug logging
#   -h, --help          Show this help
#
# Examples:
#   ./run.sh                      # No test tasks, quiet
#   ./run.sh -t 8                 # 8 tasks, quiet
#   ./run.sh -t 50 --debug        # 50 tasks, verbose
#   ./run.sh --debug              # No tasks, verbose

set -e

TASKS=0          # 0 = no dynamic test tasks
DEBUG=0          # 0 = quiet, 1 = verbose
TARGET="x86_64-unknown-none"

while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--tasks)  TASKS="$2"; shift 2 ;;
        -d|--debug)  DEBUG=1; shift ;;
        -h|--help)
            echo "Usage: $0 [-t N] [-d]"
            echo "  -t, --tasks N   Number of tasks (0 = none)"
            echo "  -d, --debug     Enable verbose debug logging"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo "Building kernel..."
cargo build --target "$TARGET" --release 2>&1 | grep -E "error|Finished" || true

echo "Running with $TASKS tasks, debug=$DEBUG ..."

# Write the KernelConfig struct (16 bytes) to physical address 0x90000.
# Each field is a u32 written via QEMU -device loader (4 bytes LE per write).
#   [0x90000] magic        = 0x5A4F4841 ("ZOHA" LE)
#   [0x90004] debug_enabled = 0 or 1
#   [0x90008] task_count    = N
#   [0x9000c] test_mode     = 0
LOADER_FLAG="-device loader,addr=0x90000,data=0x5a4f4841,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x90004,data=$DEBUG,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x90008,data=$TASKS,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x9000c,data=0,data-len=4"

qemu-system-x86_64 \
    -nographic \
    -no-reboot \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    $LOADER_FLAG \
    -kernel "target/$TARGET/release/zohara"
