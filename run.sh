#!/bin/bash
set -e

TASKS=0
DEBUG=0
VERIFY=0
SMP=1
TARGET="x86_64-unknown-none"

while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--tasks)    TASKS="$2"; shift 2 ;;
        -d|--debug)    DEBUG=1; shift ;;
        -v|--verify)   VERIFY=1; shift ;;
        -s|--smp)      SMP="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [-t N] [-d] [-v] [-s N]"
            echo "  -t, --tasks N   Number of tasks (0 = none)"
            echo "  -d, --debug     Enable verbose debug logging"
            echo "  -v, --verify    Run full verification sequence"
            echo "  -s, --smp N     Number of CPU cores (default: 1)"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo "Building kernel..."
cargo build --target "$TARGET" --release 2>&1 | grep -E "error|Finished" || true

echo "Running with $TASKS tasks, debug=$DEBUG, verify=$VERIFY, smp=$SMP ..."

DISK_IMG="zohara_disk.img"
if [ ! -f "$DISK_IMG" ]; then
    echo "Creating and formatting disk image: $DISK_IMG (32 MB, FAT32)..."
    qemu-img create -f raw "$DISK_IMG" 32M 2>&1 | tail -1
    python3 -c "
import struct
f = open('''$DISK_IMG'''', 'r+b')
# Write FAT32 BPB
bpb = bytearray(512)
bpb[0:3] = b'\xEB\x5A\x90'  # JMP
bpb[3:11] = b'ZOHARA  '  # OEM
struct.pack_into('<H', bpb, 11, 512)  # bytes per sector
bpb[13] = 1  # sectors per cluster
struct.pack_into('<H', bpb, 14, 32)  # reserved sectors
bpb[16] = 2  # number of FATs
struct.pack_into('<H', bpb, 17, 0)  # root entry count (FAT32=0)
struct.pack_into('<H', bpb, 19, 0)  # total sectors 16
bpb[21] = 0xF8  # media type
struct.pack_into('<H', bpb, 22, 0)  # FAT size 16
struct.pack_into('<H', bpb, 24, 63)  # sectors per track
struct.pack_into('<H', bpb, 26, 255)  # number of heads
struct.pack_into('<I', bpb, 28, 0)  # hidden sectors
struct.pack_into('<I', bpb, 32, 65536)  # total sectors 32 (32MB)
struct.pack_into('<I', bpb, 36, 504)  # FAT size 32
struct.pack_into('<H', bpb, 40, 0)  # ext flags
struct.pack_into('<H', bpb, 42, 0)  # fs version
struct.pack_into('<I', bpb, 44, 2)  # root cluster
struct.pack_into('<H', bpb, 48, 1)  # fs info sector
struct.pack_into('<H', bpb, 50, 6)  # backup boot sector
bpb[64] = 0x80  # drive number
bpb[66] = 0x29  # boot signature
struct.pack_into('<I', bpb, 67, 0x12345678)  # volume ID
bpb[71:82] = b'ZOHARA     '  # volume label
bpb[82:90] = b'FAT32   '  # filesystem type
bpb[510:512] = b'\x55\xAA'  # boot signature
f.seek(0)
f.write(bytes(bpb))
# Write FAT32 info sector at sector 1
info = bytearray(512)
info[0:4] = b'RRaA'
struct.pack_into('<I', info, 484, 0xFFFFFFFF)  # free cluster hint
struct.pack_into('<I', info, 488, 0xFFFFFFFF)  # last allocated cluster
info[508:512] = b'\x55\xAA'
f.seek(512)
f.write(bytes(info))
# Write FAT (2 copies)
fat = bytearray(2048)  # 4 sectors = 2048 bytes
struct.pack_into('<I', fat, 0, 0x0FFFFFF8)  # media byte + EOC
struct.pack_into('<I', fat, 4, 0x0FFFFFF8)  # cluster 1 = EOC (reserved)
struct.pack_into('<I', fat, 8, 0x0FFFFFF8)  # cluster 2 = EOC (root dir)
f.seek(32 * 512)  # FAT starts at sector 32
f.write(bytes(fat))
f.seek(32 * 512 + 2048)  # second FAT copy
f.write(bytes(fat))
f.close()
print('FAT32 formatted')
"
fi

# KernelConfig: magic(4) + debug(4) + tasks(4) + verify(4) = 16 bytes at 0x90000
LOADER_FLAG="-device loader,addr=0x90000,data=0x5a4f4841,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x90004,data=$DEBUG,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x90008,data=$TASKS,data-len=4"
LOADER_FLAG="$LOADER_FLAG -device loader,addr=0x9000c,data=$VERIFY,data-len=4"

qemu-system-x86_64 \
    -nographic \
    -no-reboot \
    -m 512 \
    -smp "$SMP" \
    -cpu qemu64,+ssse3,+sse4.1,+sse4.2 \
    -drive file="$DISK_IMG",format=raw,if=ide,index=0,media=disk \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    $LOADER_FLAG \
    -kernel "target/$TARGET/release/zohara"
