/*
 * Minimal SMP AP-wake reference test — VGA + Serial output.
 * Boots via GRUB (BIOS) from ISO or disk.
 * Sends INIT-SIPI-SIPI to CPU 1, checks if AP wakes.
 */

#include <stdint.h>

/* === Port I/O === */
static inline void outb(uint16_t port, uint8_t val) {
    __asm__ volatile("outb %0, %1" : : "a"(val), "Nd"(port));
}
static inline uint8_t inb(uint16_t port) {
    uint8_t v; __asm__ volatile("inb %1, %0" : "=a"(v) : "Nd"(port)); return v;
}

/* === VGA text mode (0xB8000) === */
static volatile uint16_t *vga = (volatile uint16_t *)0xB8000;
static int vga_row = 0;
static int vga_col = 0;
static const uint8_t vga_color = 0x07; /* white on black */

static void vga_scroll(void) {
    for (int i = 0; i < 80 * 24; i++)
        vga[i] = vga[i + 80];
    for (int i = 80 * 24; i < 80 * 25; i++)
        vga[i] = (vga_color << 8) | ' ';
}

static void vga_putc(char c) {
    if (c == '\n') {
        vga_col = 0;
        vga_row++;
    } else {
        vga[vga_row * 80 + vga_col] = (vga_color << 8) | (uint8_t)c;
        vga_col++;
    }
    if (vga_col >= 80) { vga_col = 0; vga_row++; }
    if (vga_row >= 25) { vga_scroll(); vga_row = 24; }
}

static void vga_puts(const char *s) { while (*s) vga_putc(*s++); }
static void vga_puthex(uint32_t v) {
    char buf[9]; buf[8] = 0;
    for (int i = 7; i >= 0; i--) { buf[i] = "0123456789abcdef"[v & 0xF]; v >>= 4; }
    vga_puts("0x"); int s = 0; while (s < 7 && buf[s] == '0') s++;
    vga_puts(&buf[s]);
}

/* === Serial (COM1) === */
static void serial_init(void) {
    outb(0x3F9, 0x00); outb(0x3FB, 0x80);
    outb(0x3F8, 0x01); outb(0x3F9, 0x00);
    outb(0x3FB, 0x03); outb(0x3FC, 0xC7); outb(0x3F9, 0x0B);
}
static void serial_putc(char c) {
    while ((inb(0x3FD) & 0x20) == 0) {}
    outb(0x3F8, c);
}
static void serial_puts(const char *s) { while (*s) serial_putc(*s++); }
static void serial_puthex(uint32_t v) {
    char buf[9]; buf[8] = 0;
    for (int i = 7; i >= 0; i--) { buf[i] = "0123456789abcdef"[v & 0xF]; v >>= 4; }
    serial_puts("0x"); int s = 0; while (s < 7 && buf[s] == '0') s++;
    serial_puts(&buf[s]);
}

/* === Output to both VGA and serial === */
static void print(const char *s) { vga_puts(s); serial_puts(s); }
static void puthex(uint32_t v) { vga_puthex(v); serial_puthex(v); }

/* === LAPIC === */
#define LAPIC_BASE   0xFEE00000u
#define LAPIC_ID     (LAPIC_BASE + 0x020)
#define LAPIC_ICRH   (LAPIC_BASE + 0x0C4)
#define LAPIC_ICRL   (LAPIC_BASE + 0x0C0)
#define LAPIC_SVR    (LAPIC_BASE + 0x0F0)
#define LAPIC_LVT_LINT0 (LAPIC_BASE + 0x350)
#define LAPIC_LVT_LINT1 (LAPIC_BASE + 0x354)

static inline uint32_t lapic_r(uint32_t a) { return *(volatile uint32_t *)a; }
static inline void     lapic_w(uint32_t a, uint32_t v) { *(volatile uint32_t *)a = v; }

/* Trampoline: written to 0x8000. AP starts here after SIPI vec=8.
 * Sets DS=0, uses ES=0xB800 for VGA writes.
 * Writes "AP!!" to VGA + marker 0xAABBCCDD to 0x9000, then halts. */
static const uint8_t trampoline[] = {
    0xFA,                            /* CLI */
    0x31, 0xC0,                      /* XOR AX, AX */
    0x8E, 0xD8,                      /* MOV DS, AX — DS=0 */
    0xB8, 0x00, 0xB8,               /* MOV AX, 0xB800 */
    0x8E, 0xC0,                      /* MOV ES, AX — ES=0xB800 */
    /* MOV WORD [ES:0x0000], 0x4150 — "PA" on VGA */
    0x26, 0xC7, 0x06, 0x00, 0x00, 0x50, 0x41,
    /* MOV WORD [ES:0x0020], 0x2121 — "!!" on VGA row 1 */
    0x26, 0xC7, 0x06, 0x20, 0x00, 0x21, 0x21,
    /* MOV WORD [0x9000], 0xCCDD — marker (DS=0) */
    0xC7, 0x06, 0x00, 0x90, 0xDD, 0xCC,
    /* MOV WORD [0x9002], 0xAABB — marker (DS=0) */
    0xC7, 0x06, 0x02, 0x90, 0xBB, 0xAA,
    /* HLT */
    0xF4,
};

static void delay_ms(uint32_t ms) {
    for (volatile uint32_t i = 0; i < ms; i++)
        for (volatile uint32_t j = 0; j < 200000; j++)
            __asm__ volatile("nop");
}

void main(void) {
    /* Clear VGA screen */
    for (int i = 0; i < 80 * 25; i++) vga[i] = (vga_color << 8) | ' ';

    serial_init();
    print("[SMP-REF] ====================================\n");
    print("[SMP-REF] SMP AP-Wake Reference Test\n");
    print("[SMP-REF] (BIOS boot via GRUB ISO/disk)\n");
    print("[SMP-REF] ====================================\n\n");

    /* Check if AP is running our code (shouldn't happen) */
    uint32_t my_id = lapic_r(LAPIC_ID) >> 24;
    print("[SMP-REF] This CPU LAPIC ID = "); puthex(my_id); print("\n");
    if (my_id != 0) {
        print("[SMP-REF] ERROR: AP is running main()! Halting.\n");
        for (;;) __asm__ volatile("hlt");
    }

    /* Enable LAPIC */
    lapic_w(LAPIC_SVR, 0x1FF);
    lapic_w(LAPIC_LVT_LINT0, 0x00010700);
    lapic_w(LAPIC_LVT_LINT1, 0x00010700);
    print("[SMP-REF] BSP LAPIC enabled\n");

    /* Copy trampoline to 0x8000 */
    uint8_t *dst = (uint8_t *)0x8000;
    for (uint32_t i = 0; i < sizeof(trampoline); i++) dst[i] = trampoline[i];
    print("[SMP-REF] Trampoline at 0x8000 OK\n");

    /* Clear marker and VGA "AP" marker */
    *(volatile uint32_t *)0x9000 = 0;
    /* Write "??" to VGA top-right corner so we can see if AP touches VGA */
    vga[2] = (0x4F << 8) | '?';
    vga[3] = (0x4F << 8) | '?';

    /* === Find AP's actual APIC ID from ACPI tables === */
    print("[SMP-REF] Scanning for RSDP/MADT...\n");
    uint32_t target_apic_id = 0xFFFFFFFF;

    /* Check EBDA pointer at 0x40:0x0E */
    uint16_t ebda_seg = *(volatile uint16_t *)0x40E;
    uint32_t ebda_phys = (uint32_t)ebda_seg << 4;
    if (ebda_phys > 0 && ebda_phys < 0xA0000) {
        print("[SMP-REF] EBDA at "); puthex(ebda_phys); print("\n");
        /* Scan EBDA for RSDP */
        for (uint32_t a = ebda_phys; a < ebda_phys + 0x400; a += 16) {
            if (*(uint32_t *)a == 0x20445352 && *(uint32_t *)(a+4) == 0x20525450) {
                print("[SMP-REF] Found RSDP in EBDA at "); puthex(a); print("\n");
                goto rsdp_found;
            }
        }
    }

    /* Scan BIOS ROM area 0xE0000-0xFFFFF for RSDP */
    for (uint32_t a = 0xE0000; a < 0x100000; a += 16) {
        if (*(uint32_t *)a == 0x20445352 && *(uint32_t *)(a+4) == 0x20525450) {
            print("[SMP-REF] Found RSDP at "); puthex(a); print("\n");
            goto rsdp_found;
        }
    }
    print("[SMP-REF] RSDP not found anywhere\n");
    goto no_madt;

rsdp_found:
    ;
    uint8_t *rsdp = (uint8_t *)0; /* placeholder, set below */
    /* Re-find to set pointer */
    for (uint32_t a = 0xE0000; a < 0x100000; a += 16) {
        if (*(uint32_t *)a == 0x20445352 && *(uint32_t *)(a+4) == 0x20525450) {
            rsdp = (uint8_t *)a; break;
        }
    }
    if (!rsdp && ebda_phys > 0) {
        for (uint32_t a = ebda_phys; a < ebda_phys + 0x400; a += 16) {
            if (*(uint32_t *)a == 0x20445352 && *(uint32_t *)(a+4) == 0x20525450) {
                rsdp = (uint8_t *)a; break;
            }
        }
    }
    if (!rsdp) goto no_madt;

    {
        /* Try RSDT first (32-bit pointers), then XSDT (64-bit) */
        uint32_t rsdt_addr = *(uint32_t *)(rsdp + 16);
        uint32_t xsdt_hi = *(uint32_t *)(rsdp + 20);
        if (rsdt_addr == 0 && xsdt_hi == 0) {
            /* RSDP v1: use RSDT at offset 12 */
            rsdt_addr = *(uint32_t *)(rsdp + 12);
        }
        print("[SMP-REF] RSDT/XSDT at "); puthex(rsdt_addr); print("\n");
        if (rsdt_addr == 0 || rsdt_addr > 0xF0000000) goto no_madt;

        uint32_t tbl_len = *(uint32_t *)(rsdt_addr + 4);
        uint32_t entries = (tbl_len > 36) ? (tbl_len - 36) / 4 : 0;
        print("[SMP-REF] RSDT has "); puthex(entries); print(" entries\n");

        for (uint32_t e = 0; e < entries; e++) {
            uint32_t tbl = *(uint32_t *)(rsdt_addr + 36 + e * 4);
            if (tbl == 0 || tbl > 0xF0000000) continue;
            if (*(uint32_t *)tbl == 0x43495041) { /* "APIC" */
                uint32_t madt_len = *(uint32_t *)(tbl + 4);
                uint8_t *rec = (uint8_t *)(tbl + 44);
                uint8_t *end = (uint8_t *)(tbl + madt_len);
                print("[SMP-REF] MADT found at "); puthex(tbl); print("\n");
                while (rec < end) {
                    uint8_t type = rec[0];
                    uint8_t len = rec[1];
                    if (len < 2) break;
                    if (type == 0 && len >= 8) {
                        uint8_t apic_id = rec[3];
                        uint32_t flags = *(uint32_t *)(rec + 4);
                        print("[SMP-REF] MADT CPU: apic_id=");
                        puthex(apic_id);
                        print(" flags="); puthex(flags);
                        print((flags & 1) ? " [enabled]\n" : " [disabled]\n");
                        if ((flags & 1) && apic_id != 0 && target_apic_id == 0xFFFFFFFF) {
                            target_apic_id = apic_id;
                        }
                    }
                    rec += len;
                }
                goto madt_done;
            }
        }
    }
madt_done:
no_madt:

    if (target_apic_id == 0xFFFFFFFF) {
        print("[SMP-REF] Could not find AP APIC ID from MADT, defaulting to 1\n");
        target_apic_id = 1;
    } else {
        print("[SMP-REF] AP APIC ID from MADT = "); puthex(target_apic_id); print("\n");
    }

    /* === INIT-SIPI-SIPI (exact Linux kernel sequence) === */
    print("[SMP-REF] --- INIT-SIPI-SIPI (Linux kernel method) ---\n");

    /* Diagnostic: read LAPIC state before sending IPIs */
    /* Read IA32_APIC_BASE MSR to check x2APIC mode */
    uint32_t msr_lo, msr_hi;
    __asm__ volatile("rdmsr" : "=a"(msr_lo), "=d"(msr_hi) : "c"(0x1B));
    uint64_t apic_base_msr = ((uint64_t)msr_hi << 32) | msr_lo;
    print("[SMP-REF] IA32_APIC_BASE MSR="); puthex((uint32_t)(apic_base_msr >> 32)); puthex((uint32_t)apic_base_msr); print("\n");
    print("[SMP-REF]   x2APIC enable bit(10)="); puthex((apic_base_msr >> 10) & 1); print("\n");
    print("[SMP-REF]   APIC enable bit(11)="); puthex((apic_base_msr >> 11) & 1); print("\n");
    print("[SMP-REF]   APIC base="); puthex((uint32_t)(apic_base_msr & 0xFFFFF000)); print("\n");

    print("[SMP-REF] LAPIC SVR="); puthex(lapic_r(LAPIC_SVR)); print("\n");
    print("[SMP-REF] LAPIC ID="); puthex(lapic_r(LAPIC_ID)); print("\n");
    print("[SMP-REF] ICR_LOW before INIT="); puthex(lapic_r(LAPIC_ICRL)); print("\n");
    print("[SMP-REF] ICR_HIGH before INIT="); puthex(lapic_r(LAPIC_ICRH)); print("\n");

    /* Test: write and readback ICR_LOW */
    lapic_w(LAPIC_ICRL, 0x12345678);
    print("[SMP-REF] ICR_LOW write 0x12345678, readback=0x"); puthex(lapic_r(LAPIC_ICRL)); print("\n");

    /* Wait for any pending IPI to complete */
    while (lapic_r(LAPIC_ICRL) & (1 << 12)) {}

    /* Step 1: INIT (all including self, level trigger) */
    lapic_w(LAPIC_ICRH, 0);
    lapic_w(LAPIC_ICRL, 0x000C8500);
    /* Read back to verify write was accepted */
    print("[SMP-REF] Step1: INIT (0xC8500)\n");
    print("[SMP-REF]   ICR after INIT write=0x"); puthex(lapic_r(LAPIC_ICRL)); print("\n");

    while (lapic_r(LAPIC_ICRL) & (1 << 12)) {}
    delay_ms(10);

    /* Step 2: SIPI #1 (all including self, edge, vector=0x08) */
    lapic_w(LAPIC_ICRH, 0);
    lapic_w(LAPIC_ICRL, 0x000C0608);
    print("[SMP-REF] Step2: SIPI #1 (0xC0608)\n");
    print("[SMP-REF]   ICR after SIPI write=0x"); puthex(lapic_r(LAPIC_ICRL)); print("\n");

    while (lapic_r(LAPIC_ICRL) & (1 << 12)) {}
    delay_ms(10);

    /* Step 3: SIPI #2 */
    lapic_w(LAPIC_ICRH, 0);
    lapic_w(LAPIC_ICRL, 0x000C0608);
    print("[SMP-REF] Step3: SIPI #2 (0xC0608)\n");

    while (lapic_r(LAPIC_ICRL) & (1 << 12)) {}
    delay_ms(10);

    /* Poll for marker */
    print("[SMP-REF] Polling marker at 0x9000...\n");
    int ok = 0;
    for (int i = 0; i < 500; i++) {
        delay_ms(10);
        if (*(volatile uint32_t *)0x9000 == 0xAABBCCDD) {
            print("[SMP-REF] Marker found!\n");
            ok = 1; break;
        }
        if (i == 100) print("[SMP-REF] ...still waiting...\n");
        if (i == 250) print("[SMP-REF] ...more waiting...\n");
    }

    print("\n");
    if (ok) {
        print("========================================\n");
        print("  >>> PASS <<<  AP woke up!\n");
        print("========================================\n");
    } else {
        print("========================================\n");
        print("  >>> FAIL <<<  AP did NOT wake up.\n");
        print("========================================\n");
    }

    /* isa-debug-exit (for QEMU) */
    *(volatile uint32_t *)0xF4 = ok ? 0x10 : 0x31;
    for (;;) __asm__ volatile("hlt");
}
