# Minimal SMP AP-wake reference test
CC = gcc
LD = ld

GCC_INC := $(shell $(CC) -m32 -print-file-name=include)
CFLAGS = -m32 -ffreestanding -nostdlib -nostdinc -isystem $(GCC_INC) -fno-builtin -fno-stack-protector \
         -Wall -O2 -mno-sse -mno-sse2 -mno-mmx
ASFLAGS = -m32 -ffreestanding -nostdlib
LDFLAGS = -m elf_i386 -nostdlib -T link.ld

all: ref_test.elf

boot.o: boot.S
	gcc $(ASFLAGS) -c -o $@ $<

ref_test.o: ref_test.c
	$(CC) $(CFLAGS) -c -o $@ $<

ref_test.elf: boot.o ref_test.o link.ld
	$(LD) $(LDFLAGS) -o $@ boot.o ref_test.o

clean:
	rm -f boot.o ref_test.o ref_test.elf

.PHONY: all clean
